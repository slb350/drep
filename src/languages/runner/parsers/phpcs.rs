//! PHP_CodeSniffer's `--report=json`: files keyed by path, not listed.

use serde_json::Value;

use crate::analysis::findings::{Finding, Severity};
use crate::languages::runner::ToolOutputError;
use crate::languages::spec::ToolSpec;

use super::{expect_keyed_object, json_payload};

/// `phpcs --report=json`: `{files: {"/abs/path.php": {messages: [...]}}}`.
///
/// The `files` value is an OBJECT keyed by path - phpcs is the one checker
/// here that maps rather than lists - and the keys are absolute in real
/// output. They are passed through unchanged: `check` resolves absolute
/// reported paths itself, and re-relativising here would duplicate that
/// logic with a different cwd assumption.
pub(super) fn parse_phpcs(spec: &ToolSpec, output: &str) -> Result<Vec<Finding>, ToolOutputError> {
    let Some(payload) = json_payload(spec, output)? else {
        return Ok(Vec::new());
    };
    let files = expect_keyed_object(spec, &payload, "files")?;

    let mut findings = Vec::new();
    for (path, file) in files {
        for message in file
            .get("messages")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            findings.push(Finding::deterministic(
                message
                    .get("source")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .unwrap_or_else(|| spec.name.to_owned()),
                phpcs_severity(message.get("type").and_then(Value::as_str)),
                path.clone(),
                message
                    .get("line")
                    .and_then(Value::as_u64)
                    .map(|n| n as u32)
                    .unwrap_or(1),
                message
                    .get("column")
                    .and_then(Value::as_u64)
                    .map(|n| n as u32),
                message
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
                None,
            ));
        }
    }
    Ok(findings)
}

/// phpcs `type` to drep severity.
///
/// Compared case-insensitively because phpcs emits `ERROR`/`WARNING` in caps
/// while the report format has carried both spellings across releases.
/// `warning` falls to the catch-all Warning, which is also where an
/// absent or unrecognised `type` lands; the default is safe because phpcs
/// has only ever spelled these two values, and a value it adds later is
/// still a finding the project asked to hear about.
/// The numeric `severity` field is deliberately never read: it is a phpcs
/// *priority* (1-10, how loudly a sniff reports), not a level, and mapping it
/// onto severity would let a project's priority tuning decide what gates.
fn phpcs_severity(kind: Option<&str>) -> Severity {
    match kind {
        Some(value) if value.eq_ignore_ascii_case("error") => Severity::Error,
        _ => Severity::Warning,
    }
}
