//! Fast, redacted Codex CLI readiness check via `--version` and `login status`.
//!
//! `codex doctor --json` runs full SQLite integrity and session-rollout parity
//! scans that can exceed two minutes on machines with large Codex histories,
//! whereas drep only needs two redacted facts: the CLI version (for cache
//! identity and reporting) and confirmation that ChatGPT-managed auth is
//! active. `--version` and `login status` answer both in ~0.1 s without reading
//! Codex's credential files or history databases directly.

use std::ffi::OsStr;
use std::path::Path;
use std::process::{ExitStatus, Stdio};
use std::time::{Duration, Instant};

use thiserror::Error;

use super::{
    CodexStatus,
    capture::{BoundedCapture, ReadBoundedError},
    command::{ChildEnvironment, toml_override},
};

const DIAGNOSTIC_MAX_BYTES: usize = 1024 * 1024;
pub(crate) const DIAGNOSTIC_TIMEOUT: Duration = Duration::from_secs(30);
const VERSION_PREFIX: &str = "codex-cli ";
const CHATGPT_LOGIN_LINE: &str = "Logged in using ChatGPT";

#[derive(Debug, Error, PartialEq, Eq)]
pub(crate) enum DiagnosticError {
    #[error("Codex CLI was not found on PATH; install it and run `codex login`")]
    MissingBinary,
    #[error("could not run the Codex CLI diagnostic")]
    Process,
    #[error("could not create an isolated Codex CLI diagnostic workspace")]
    Workspace,
    #[error("Codex CLI diagnostic timed out")]
    Timeout,
    #[error("Codex CLI diagnostic output was too large")]
    OutputTooLarge,
    #[error("unsupported Codex CLI diagnostic format; update drep or use a supported Codex CLI")]
    UnsupportedFormat,
    #[error("Codex authentication is not ChatGPT-managed; run `codex login`")]
    NotChatGpt,
}

/// Verify Codex CLI version and ChatGPT-managed authentication in an empty
/// directory, retaining only the redacted CLI version.
pub(crate) fn probe(
    executable: &Path,
    environment: &ChildEnvironment,
    timeout: Duration,
) -> Result<CodexStatus, DiagnosticError> {
    let cwd = tempfile::tempdir().map_err(|_| DiagnosticError::Workspace)?;
    let started = Instant::now();

    let (version_status, version_bytes) = run_capture(
        executable,
        &[OsStr::new("--version")],
        environment,
        cwd.path(),
        false,
        started,
        timeout,
    )?;
    if !version_status.success() {
        return Err(DiagnosticError::Process);
    }
    let version = parse_version(&version_bytes)?;

    let forced_login = toml_override("forced_login_method", "chatgpt");
    let (login_status, login_bytes) = run_capture(
        executable,
        &[
            OsStr::new("-c"),
            OsStr::new(&forced_login),
            OsStr::new("login"),
            OsStr::new("status"),
        ],
        environment,
        cwd.path(),
        true,
        started,
        timeout,
    )?;
    parse_login_status(login_status.success(), &login_bytes)?;

    Ok(CodexStatus::new(version))
}

fn run_capture(
    executable: &Path,
    args: &[&OsStr],
    environment: &ChildEnvironment,
    cwd: &Path,
    merge_stderr: bool,
    started: Instant,
    timeout: Duration,
) -> Result<(ExitStatus, Vec<u8>), DiagnosticError> {
    let mut output = BoundedCapture::new().map_err(|_| DiagnosticError::Workspace)?;
    let child_stdout = output
        .child_stdio()
        .map_err(|_| DiagnosticError::Workspace)?;
    let child_stderr = if merge_stderr {
        output
            .child_stdio()
            .map_err(|_| DiagnosticError::Workspace)?
    } else {
        Stdio::null()
    };
    let mut command = std::process::Command::new(executable);
    command
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(child_stdout)
        .stderr(child_stderr);
    environment.apply_to_std(&mut command);
    let mut child = command.spawn().map_err(|err| match err.kind() {
        std::io::ErrorKind::NotFound => DiagnosticError::MissingBinary,
        _ => DiagnosticError::Process,
    })?;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if poll_before_deadline(started.elapsed(), timeout) => {
                if output.exceeds(DIAGNOSTIC_MAX_BYTES).unwrap_or(true) {
                    stop(&mut child);
                    return Err(DiagnosticError::OutputTooLarge);
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Ok(None) => {
                stop(&mut child);
                return Err(DiagnosticError::Timeout);
            }
            Err(_) => {
                stop(&mut child);
                return Err(DiagnosticError::Process);
            }
        }
    };
    let bytes = output
        .read_bounded(DIAGNOSTIC_MAX_BYTES)
        .map_err(|err| match err {
            ReadBoundedError::TooLarge(_) => DiagnosticError::OutputTooLarge,
            ReadBoundedError::Io(_) => DiagnosticError::Process,
        })?;
    Ok((status, bytes))
}

pub(super) fn stop(child: &mut std::process::Child) {
    let _ = child.kill();
    let _ = child.wait();
}

/// Whether another diagnostic poll fits before the deadline.
///
/// Kept as a pure decision so the equality boundary is deterministic in tests;
/// a wall-clock test cannot arrange for `Instant::elapsed()` to equal a duration
/// exactly.
pub(crate) fn poll_before_deadline(elapsed: Duration, deadline: Duration) -> bool {
    elapsed < deadline
}

/// Parse `codex --version` output (`codex-cli <version>`), rejecting empty,
/// whitespace-bearing, or control-character-bearing versions so untrusted bytes
/// can never reach terminal output.
pub(crate) fn parse_version(input: &[u8]) -> Result<String, DiagnosticError> {
    let text = std::str::from_utf8(input).map_err(|_| DiagnosticError::UnsupportedFormat)?;
    let version = text
        .trim_end_matches(['\r', '\n'])
        .strip_prefix(VERSION_PREFIX)
        .ok_or(DiagnosticError::UnsupportedFormat)?;
    if version.is_empty()
        || version
            .chars()
            .any(|ch| ch.is_control() || ch.is_whitespace())
    {
        return Err(DiagnosticError::UnsupportedFormat);
    }
    Ok(version.to_owned())
}

/// Interpret `codex login status` without retaining raw account details.
///
/// Codex prints `Logged in using ChatGPT` on stderr (or stdout in future
/// releases) when ChatGPT-managed login is active. API-key or logged-out states
/// map to [`DiagnosticError::NotChatGpt`]; an empty nonzero exit is a process
/// failure.
pub(crate) fn parse_login_status(success: bool, input: &[u8]) -> Result<(), DiagnosticError> {
    if !success && input.is_empty() {
        return Err(DiagnosticError::Process);
    }
    let text = std::str::from_utf8(input).map_err(|_| DiagnosticError::UnsupportedFormat)?;
    if success && text.lines().any(|line| line.trim() == CHATGPT_LOGIN_LINE) {
        return Ok(());
    }
    Err(DiagnosticError::NotChatGpt)
}
