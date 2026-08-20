//! Bounded, timed execution of one isolated Codex child.

use std::ffi::OsString;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use tokio::io::AsyncWriteExt;

use crate::llm::error::LlmError;
use crate::text::excerpt;

use super::{capture::BoundedCapture, command::ChildEnvironment};

const STDOUT_MAX_BYTES: usize = 16 * 1024 * 1024;
const STDERR_MAX_BYTES: usize = 32 * 1024;
const CAPTURE_FILE_MAX_BYTES: usize = STDOUT_MAX_BYTES;
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
    let mut stdout = BoundedCapture::new().map_err(capture_setup_error)?;
    let mut stderr = BoundedCapture::new().map_err(capture_setup_error)?;
    let child_stdout = stdout.child_stdio().map_err(capture_setup_error)?;
    let child_stderr = stderr.child_stdio().map_err(capture_setup_error)?;
    let mut command = tokio::process::Command::new(executable);
    command
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(child_stdout)
        .stderr(child_stderr)
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
    let send_input = async move {
        stdin.write_all(input.as_bytes()).await?;
        stdin.shutdown().await?;
        Ok::<(), std::io::Error>(())
    };
    // Capture into private files rather than pipes. A direct child can exit
    // after spawning a grandchild that inherited stdout/stderr; waiting for
    // pipe EOF then waits for the unrelated grandchild too. Regular files have
    // no EOF handshake, while the size poll below keeps them bounded.
    let execution = async {
        let mut completion = std::pin::pin!(async { tokio::join!(child.wait(), send_input) });
        let mut size_poll = tokio::time::interval(Duration::from_millis(10));
        size_poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                results = &mut completion => return Ok::<_, LlmError>(results),
                _ = size_poll.tick() => {
                    check_capture_size("stdout", &stdout, STDOUT_MAX_BYTES)?;
                    check_capture_size("stderr", &stderr, CAPTURE_FILE_MAX_BYTES)?;
                }
            }
        }
    };
    let (status, stdin_result) = match tokio::time::timeout(timeout, execution).await {
        Ok(Ok(results)) => results,
        Ok(Err(err)) => {
            stop(&mut child).await;
            return Err(err);
        }
        Err(_) => {
            stop(&mut child).await;
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
    check_capture_size("stdout", &stdout, STDOUT_MAX_BYTES)?;
    check_capture_size("stderr", &stderr, CAPTURE_FILE_MAX_BYTES)?;
    let stdout = stdout
        .read_bounded(STDOUT_MAX_BYTES)
        .map_err(|err| stream_error("stdout", err))?;
    let stderr = stderr
        .read_prefix(STDERR_MAX_BYTES)
        .map_err(|err| stream_error("stderr", err))?;
    // A nonzero child status is the authoritative failure: its JSONL/stderr
    // diagnostic explains why it stopped and closing stdin early is a normal
    // consequence. On a successful exit, a write failure still proves the
    // requested payload was not delivered in full.
    if status.success() {
        stdin_result.map_err(|err| LlmError::Transport {
            status: None,
            message: format!("could not send the review payload to Codex: {err}"),
        })?;
    }

    Ok(ProcessOutput {
        stdout,
        stderr,
        status,
    })
}

async fn stop(child: &mut tokio::process::Child) {
    let _ = child.kill().await;
    let _ = child.wait().await;
}

fn stream_error(stream: &str, err: std::io::Error) -> LlmError {
    LlmError::Transport {
        status: None,
        message: format!("could not read Codex {stream}: {err}"),
    }
}

fn capture_setup_error(err: std::io::Error) -> LlmError {
    LlmError::Transport {
        status: None,
        message: format!("could not create bounded Codex output capture: {err}"),
    }
}

fn check_capture_size(stream: &str, file: &BoundedCapture, limit: usize) -> Result<(), LlmError> {
    if file
        .exceeds(limit)
        .map_err(|err| stream_error(stream, err))?
    {
        return Err(stream_error(
            stream,
            std::io::Error::other(format!("Codex output exceeded {limit} bytes")),
        ));
    }
    Ok(())
}
