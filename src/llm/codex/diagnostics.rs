//! Redacted interpretation of `codex doctor --json`.

use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};

use serde::Deserialize;
use thiserror::Error;

use super::{CodexStatus, capture::BoundedCapture, command::ChildEnvironment};

const DIAGNOSTIC_MAX_BYTES: usize = 1024 * 1024;
pub(crate) const DIAGNOSTIC_TIMEOUT: Duration = Duration::from_secs(30);

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
    #[error("Codex diagnostic output is not valid JSON")]
    InvalidJson,
    #[error("unsupported Codex CLI diagnostic format; update drep or use a supported Codex CLI")]
    UnsupportedFormat,
    #[error("Codex authentication is not ChatGPT-managed; run `codex login`")]
    NotChatGpt,
}

/// Run the redacted diagnostic in an empty directory and retain no raw details.
pub(crate) fn probe(
    executable: &Path,
    environment: &ChildEnvironment,
    timeout: Duration,
) -> Result<CodexStatus, DiagnosticError> {
    let cwd = tempfile::tempdir().map_err(|_| DiagnosticError::Workspace)?;
    let mut output = BoundedCapture::new().map_err(|_| DiagnosticError::Workspace)?;
    let child_output = output
        .child_stdio()
        .map_err(|_| DiagnosticError::Workspace)?;
    let mut command = std::process::Command::new(executable);
    command
        .args(["doctor", "--json"])
        .current_dir(cwd.path())
        .stdin(Stdio::null())
        .stdout(child_output)
        .stderr(Stdio::null());
    environment.apply_to_std(&mut command);
    let mut child = command.spawn().map_err(|err| match err.kind() {
        std::io::ErrorKind::NotFound => DiagnosticError::MissingBinary,
        _ => DiagnosticError::Process,
    })?;
    let started = Instant::now();
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
    if output
        .exceeds(DIAGNOSTIC_MAX_BYTES)
        .map_err(|_| DiagnosticError::Process)?
    {
        return Err(DiagnosticError::OutputTooLarge);
    }
    let bytes = output
        .read_bounded(DIAGNOSTIC_MAX_BYTES)
        .map_err(|_| DiagnosticError::Process)?;
    // `codex doctor` reports unrelated health checks too and may exit nonzero
    // while still emitting a complete, known authentication document. drep
    // retains only those redacted facts, so a valid document is authoritative;
    // a nonzero status with no document is still a process failure.
    if !status.success() && bytes.is_empty() {
        return Err(DiagnosticError::Process);
    }
    parse(bytes.as_slice())
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

/// Parse a known diagnostic schema without retaining account paths or details.
pub(crate) fn parse(input: &[u8]) -> Result<CodexStatus, DiagnosticError> {
    let diagnostic: Diagnostic<'_> =
        serde_json::from_slice(input).map_err(|_| DiagnosticError::InvalidJson)?;

    if diagnostic.schema_version != Some(1) {
        return Err(DiagnosticError::UnsupportedFormat);
    }
    let version = diagnostic
        .codex_version
        .filter(|version| !version.is_empty())
        .ok_or(DiagnosticError::UnsupportedFormat)?;
    let details = diagnostic
        .checks
        .and_then(|checks| checks.auth_credentials)
        .and_then(|check| check.details)
        .ok_or(DiagnosticError::UnsupportedFormat)?;

    let chatgpt = details.stored_chatgpt_tokens == Some("true")
        && details.stored_auth_mode == Some("chatgpt");
    if !chatgpt {
        return Err(DiagnosticError::NotChatGpt);
    }

    Ok(CodexStatus::new(version))
}

#[derive(Deserialize)]
struct Diagnostic<'a> {
    #[serde(rename = "schemaVersion")]
    schema_version: Option<u64>,
    #[serde(rename = "codexVersion")]
    codex_version: Option<&'a str>,
    #[serde(borrow)]
    checks: Option<Checks<'a>>,
}

#[derive(Deserialize)]
struct Checks<'a> {
    #[serde(rename = "auth.credentials")]
    #[serde(borrow)]
    auth_credentials: Option<AuthCheck<'a>>,
}

#[derive(Deserialize)]
struct AuthCheck<'a> {
    #[serde(borrow)]
    details: Option<AuthDetails<'a>>,
}

#[derive(Deserialize)]
struct AuthDetails<'a> {
    #[serde(rename = "stored ChatGPT tokens")]
    stored_chatgpt_tokens: Option<&'a str>,
    #[serde(rename = "stored auth mode")]
    stored_auth_mode: Option<&'a str>,
}
