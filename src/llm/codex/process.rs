//! Bounded, timed execution of one isolated Codex child.

use std::ffi::OsString;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};

use crate::llm::error::LlmError;
use crate::text::excerpt;

use super::command::ChildEnvironment;

const STDOUT_MAX_BYTES: usize = 16 * 1024 * 1024;
const STDERR_MAX_BYTES: usize = 32 * 1024;
const STDERR_EXCERPT_CHARS: usize = 400;

pub(crate) struct ProcessOutput {
    pub(crate) stdout: Vec<u8>,
    pub(crate) stderr: Vec<u8>,
    pub(crate) status: std::process::ExitStatus,
}

impl ProcessOutput {
    pub(crate) fn stderr_excerpt(&self) -> String {
        excerpt(&String::from_utf8_lossy(&self.stderr), STDERR_EXCERPT_CHARS)
    }
}

pub(crate) async fn run(
    executable: &Path,
    args: &[OsString],
    environment: &ChildEnvironment,
    cwd: &Path,
    input: &str,
    timeout: Duration,
) -> Result<ProcessOutput, LlmError> {
    let mut command = tokio::process::Command::new(executable);
    command
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    environment.apply_to(&mut command);

    let mut child = command.spawn().map_err(|err| match err.kind() {
        std::io::ErrorKind::NotFound => {
            LlmError::NotConfigured("Codex CLI was not found on PATH".to_owned())
        }
        _ => LlmError::Transport {
            status: None,
            message: format!("could not start Codex CLI: {err}"),
        },
    })?;
    let mut stdin = child.stdin.take().expect("piped stdin");
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    let send_input = async move {
        stdin.write_all(input.as_bytes()).await?;
        stdin.shutdown().await?;
        drop(stdin);
        Ok::<(), std::io::Error>(())
    };
    // stderr is diagnostic only. Keep a prefix but continue draining it so a
    // noisy child cannot fill its pipe and deadlock before exit.
    let execution = async {
        tokio::join!(
            child.wait(),
            send_input,
            read_bounded(stdout, STDOUT_MAX_BYTES),
            read_capped(stderr, STDERR_MAX_BYTES),
        )
    };
    let (status, stdin_result, stdout, stderr) =
        match tokio::time::timeout(timeout, execution).await {
            Ok(results) => results,
            Err(_) => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                return Err(LlmError::Transport {
                    status: None,
                    message: format!("Codex CLI timed out after {} seconds", timeout.as_secs()),
                });
            }
        };

    let status = status.map_err(|err| LlmError::Transport {
        status: None,
        message: format!("could not wait for Codex CLI: {err}"),
    })?;
    let stdout = stdout.map_err(|err| stream_error("stdout", err))?;
    let stderr = stderr.map_err(|err| stream_error("stderr", err))?;
    stdin_result.map_err(|err| LlmError::Transport {
        status: None,
        message: format!("could not send the review payload to Codex: {err}"),
    })?;

    Ok(ProcessOutput {
        stdout,
        stderr,
        status,
    })
}

fn stream_error(stream: &str, err: std::io::Error) -> LlmError {
    LlmError::Transport {
        status: None,
        message: format!("could not read Codex {stream}: {err}"),
    }
}

async fn read_bounded(
    reader: impl AsyncRead + Unpin,
    limit: usize,
) -> Result<Vec<u8>, std::io::Error> {
    let mut bytes = Vec::new();
    reader
        .take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .await?;
    if bytes.len() > limit {
        return Err(std::io::Error::other(format!(
            "Codex output exceeded {limit} bytes"
        )));
    }
    Ok(bytes)
}

async fn read_capped(
    mut reader: impl AsyncRead + Unpin,
    limit: usize,
) -> Result<Vec<u8>, std::io::Error> {
    let mut retained = Vec::new();
    let mut chunk = [0_u8; 8192];
    loop {
        let read = reader.read(&mut chunk).await?;
        if read == 0 {
            return Ok(retained);
        }
        let keep = read.min(limit.saturating_sub(retained.len()));
        retained.extend_from_slice(&chunk[..keep]);
    }
}
