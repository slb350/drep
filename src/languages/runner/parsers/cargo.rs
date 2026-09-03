//! `cargo --message-format json`: one JSON document per line, of which only
//! the `compiler-message` events carry diagnostics.

use serde_json::Value;

use crate::analysis::findings::{Finding, Severity};
use crate::languages::runner::ToolOutputError;
use crate::languages::spec::ToolSpec;

/// cargo's newline-delimited JSON: one object per event, not one array.
///
/// Only `compiler-message` events are diagnostics; the rest are build
/// progress. A line that is not JSON at all is an **error** rather than a
/// skip, since that means we are not reading what we think we are.
pub(super) fn parse_cargo(spec: &ToolSpec, output: &str) -> Result<Vec<Finding>, ToolOutputError> {
    let mut findings = Vec::new();
    for line in output.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let event: Value = serde_json::from_str(line).map_err(|err| {
            ToolOutputError(format!("{} emitted a non-JSON line: {err}", spec.name))
        })?;

        if event.get("reason").and_then(Value::as_str) != Some("compiler-message") {
            continue;
        }

        let message = event.get("message");
        // First primary span. With cargo's output exactly one span per
        // diagnostic is primary, and array order is preserved.
        let primary = message
            .and_then(|m| m.get("spans"))
            .and_then(Value::as_array)
            .and_then(|spans| {
                spans
                    .iter()
                    .find(|s| s.get("is_primary") == Some(&Value::Bool(true)))
            });

        let kind = message
            .and_then(|m| m.get("code"))
            .and_then(|c| c.get("code"))
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| spec.name.to_owned());

        let file_path = primary
            .and_then(|p| p.get("file_name"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        let line = primary
            .and_then(|p| p.get("line_start"))
            .and_then(Value::as_u64)
            .map(|n| n as u32)
            .unwrap_or(1);
        let column = primary
            .and_then(|p| p.get("column_start"))
            .and_then(Value::as_u64)
            .map(|n| n as u32);
        let message_text = message
            .and_then(|m| m.get("message"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();

        findings.push(Finding::deterministic(
            kind,
            cargo_severity(message.and_then(|m| m.get("level")).and_then(Value::as_str)),
            file_path,
            line,
            column,
            message_text,
            None,
        ));
    }
    Ok(findings)
}

/// cargo's diagnostic `level` to drep severity.
///
/// It was hardcoded to Error, so a clippy warning displayed as an error. That
/// never changed the gate - `any_blocking_tool_finding` blocks on any tool
/// finding whatever its severity, because the tool is the project's own choice
/// - but it made the rendered line say something the compiler did not.
///
/// `note` and `help` are cargo's sub-diagnostics; they arrive as their own
/// messages only when a lint is configured to emit them standalone, and they
/// are not defects on their own. `failure-note` is: it reports the build
/// itself failing.
fn cargo_severity(level: Option<&str>) -> Severity {
    match level {
        Some("warning") => Severity::Warning,
        Some("note") | Some("help") => Severity::Info,
        _ => Severity::Error,
    }
}
