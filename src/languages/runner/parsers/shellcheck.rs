//! ShellCheck's `-f json`: a flat array of diagnostics.

use serde_json::Value;

use crate::analysis::findings::{Finding, Severity};
use crate::languages::runner::ToolOutputError;
use crate::languages::spec::ToolSpec;

use super::{expect_array, json_payload};

/// `shellcheck -f json`: one flat record per diagnostic.
///
/// `code` is a number on the wire but `SC`-prefixed in every place a user
/// meets a ShellCheck rule - the tool's own wiki, suppressions, CI logs - so
/// the finding `kind` carries the joined form. Sorting by `level` is the
/// tool's own output order and is preserved.
pub(super) fn parse_shellcheck(
    spec: &ToolSpec,
    output: &str,
) -> Result<Vec<Finding>, ToolOutputError> {
    let Some(payload) = json_payload(spec, output)? else {
        return Ok(Vec::new());
    };
    let entries = expect_array(spec, &payload)?;

    let mut findings = Vec::new();
    for entry in entries {
        let code = entry.get("code").and_then(Value::as_u64);
        findings.push(Finding::deterministic(
            code.map(|n| format!("SC{n}"))
                .unwrap_or_else(|| spec.name.to_owned()),
            shellcheck_severity(entry.get("level").and_then(Value::as_str)),
            entry
                .get("file")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned(),
            entry
                .get("line")
                .and_then(Value::as_u64)
                .map(|n| n as u32)
                .unwrap_or(1),
            entry
                .get("column")
                .and_then(Value::as_u64)
                .map(|n| n as u32),
            entry
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned(),
            None,
        ));
    }
    Ok(findings)
}

/// ShellCheck `level` to drep severity.
///
/// `style` is ShellCheck's lowest rung - guidance about phrasing and quoting
/// conventions rather than a defect - so it lands on Info with `info`.
/// `warning` is the catch-all, which also takes any level ShellCheck adds
/// later: an unknown level is something the tool wanted to say, and Info
/// would let it slip a gate that blocks on warnings.
fn shellcheck_severity(level: Option<&str>) -> Severity {
    match level {
        Some("error") => Severity::Error,
        Some("info") | Some("style") => Severity::Info,
        _ => Severity::Warning,
    }
}
