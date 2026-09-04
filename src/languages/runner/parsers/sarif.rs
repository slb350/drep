//! SARIF 2.1.0 `runs[].results[]`.
//!
//! One module for every producer of the format rather than one per tool:
//! checkstyle, cppcheck, SwiftLint, tflint and hadolint all emit it, so a
//! further SARIF producer is a table entry and not a parser.

use serde_json::Value;

use crate::analysis::findings::{Finding, Severity};
use crate::languages::runner::ToolOutputError;
use crate::languages::runner::uri::strip_file_uri;
use crate::languages::spec::ToolSpec;

/// SARIF 2.1.0: `runs[].results[]`, the interchange format checkstyle emits.
///
/// Written against the spec rather than against checkstyle, because SARIF is
/// what the rest of the JVM linters converge on and a second one should need
/// no parser. Empty input is a clean run for the same reason it is under
/// `json`; anything else that will not parse is an error.
pub(super) fn parse_sarif(spec: &ToolSpec, output: &str) -> Result<Vec<Finding>, ToolOutputError> {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    let payload: Value = serde_json::from_str(trimmed).map_err(|err| {
        ToolOutputError(format!("{} produced unparseable SARIF: {err}", spec.name))
    })?;

    let runs = payload
        .get("runs")
        .and_then(Value::as_array)
        .ok_or_else(|| ToolOutputError(format!("{}: SARIF has no runs array", spec.name)))?;

    let mut findings = Vec::new();
    for run in runs {
        for result in run
            .get("results")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let kind = result
                .get("ruleId")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .unwrap_or_else(|| spec.name.to_owned());
            let message = result
                .get("message")
                .and_then(|m| m.get("text"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();

            // First location only. SARIF allows several, but every checker
            // here reports one per diagnostic, and reporting the same finding
            // once per location would double-count a gate.
            //
            // A result with no usable artifact uri is not a finding: the tool
            // is talking about the run, not the code. tflint is the case that
            // forced this - it reports runtime errors (plugins never
            // installed, arguments it dropped) as a `tflint-errors` run whose
            // results carry no `locations`, verified against its real output.
            // Read as a finding, the empty path matches nothing the run was
            // asked about and narrowing drops it without a trace, so a tool
            // that never examined a file reports every file clean. Error
            // instead: we do not know what else the tool would have said.
            let physical = result
                .get("locations")
                .and_then(Value::as_array)
                .and_then(|locations| locations.first())
                .and_then(|location| location.get("physicalLocation"));
            let file_path = physical
                .and_then(|p| p.get("artifactLocation"))
                .and_then(|a| a.get("uri"))
                .and_then(Value::as_str)
                .map(strip_file_uri)
                .ok_or_else(|| {
                    ToolOutputError(format!(
                        "{} reported a diagnostic with no file location: {}",
                        spec.name,
                        crate::text::excerpt(&format!("{kind}: {message}"), 120)
                    ))
                })?;
            let region = physical.and_then(|p| p.get("region"));
            let line = region
                .and_then(|r| r.get("startLine"))
                .and_then(Value::as_u64)
                .map(|n| n as u32)
                .unwrap_or(1);
            let column = region
                .and_then(|r| r.get("startColumn"))
                .and_then(Value::as_u64)
                .map(|n| n as u32);

            findings.push(Finding::deterministic(
                kind,
                sarif_severity(result.get("level").and_then(Value::as_str)),
                file_path,
                line,
                column,
                message,
                None,
            ));
        }
    }
    Ok(findings)
}

/// SARIF `level` to drep severity.
///
/// `none` is SARIF for "this rule was evaluated and had nothing to say", which
/// no tool should emit as a result, so it lands on Info rather than inventing
/// a fourth level. An absent level defaults to `warning` per the spec.
fn sarif_severity(level: Option<&str>) -> Severity {
    match level {
        Some("error") => Severity::Error,
        Some("note") | Some("none") => Severity::Info,
        _ => Severity::Warning,
    }
}
