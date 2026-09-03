//! Credo's `--format json`: a flat array of issues under one key.

use serde_json::Value;

use crate::analysis::findings::{Finding, Severity};
use crate::languages::runner::ToolOutputError;
use crate::languages::spec::ToolSpec;

use super::{expect_keyed_array, json_payload};

/// `mix credo --format json`: `{issues: [...]}`.
pub(super) fn parse_credo(spec: &ToolSpec, output: &str) -> Result<Vec<Finding>, ToolOutputError> {
    let Some(payload) = json_payload(spec, output)? else {
        return Ok(Vec::new());
    };
    let issues = expect_keyed_array(spec, &payload, "issues")?;

    let mut findings = Vec::new();
    for issue in issues {
        // `column` is genuinely null when a check has no column to point at,
        // and `Value::as_u64` answers None for it - which is the answer we
        // want. Coercing null to Some(0) would put a caret where there is
        // no character.
        let column = issue
            .get("column")
            .and_then(Value::as_u64)
            .map(|n| n as u32);
        findings.push(Finding::deterministic(
            issue
                .get("check")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .unwrap_or_else(|| spec.name.to_owned()),
            credo_severity(issue.get("category").and_then(Value::as_str)),
            issue
                .get("filename")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned(),
            issue
                .get("line_no")
                .and_then(Value::as_u64)
                .map(|n| n as u32)
                .unwrap_or(1),
            column,
            issue
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned(),
            None,
        ));
    }
    Ok(findings)
}

/// Credo `category` to drep severity.
///
/// Credo has no severity field; its categories rank how mechanical a fix is,
/// not how bad the code is. `warning` is the exception - it is Credo's
/// correctness bucket (unused results, operators on the wrong type), which
/// is exactly what should gate a commit, so it maps *up* to Error.
/// `readability` and `consistency` are style. `refactor` and `design` fall
/// to the catch-all Warning, which is also where everything unrecognised
/// lands: the catch-all is safe because those categories flag structure
/// worth changing but not proven defects, the same judgment an unknown
/// category deserves.
fn credo_severity(category: Option<&str>) -> Severity {
    match category {
        Some("warning") => Severity::Error,
        Some("readability") | Some("consistency") => Severity::Info,
        _ => Severity::Warning,
    }
}
