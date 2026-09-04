//! RuboCop's `--format json`: an object of per-file offense lists.

use serde_json::Value;

use crate::analysis::findings::{Finding, Severity};
use crate::languages::runner::ToolOutputError;
use crate::languages::spec::ToolSpec;

use super::{expect_keyed_array, json_payload, path_or_root};

/// `rubocop --format json`: `{files: [{path, offenses: [...]}]}`.
///
/// The line and column are read from `location.start_line`/`start_column`
/// with a fallback to `location.line`/`column`: current RuboCop
/// emits both, but `start_line` is the documented key and older releases
/// wrote only `line`.
pub(super) fn parse_rubocop(
    spec: &ToolSpec,
    output: &str,
    root_name: &str,
) -> Result<Vec<Finding>, ToolOutputError> {
    let Some(payload) = json_payload(spec, output)? else {
        return Ok(Vec::new());
    };
    let files = expect_keyed_array(spec, &payload, "files")?;

    let mut findings = Vec::new();
    for file in files {
        let file_path = path_or_root(file.get("path"), root_name);
        for offense in file
            .get("offenses")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let location = offense.get("location");
            findings.push(Finding::deterministic(
                offense
                    .get("cop_name")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .unwrap_or_else(|| spec.name.to_owned()),
                rubocop_severity(offense.get("severity").and_then(Value::as_str)),
                file_path.clone(),
                location
                    .and_then(|l| l.get("start_line").or_else(|| l.get("line")))
                    .and_then(Value::as_u64)
                    .map(|n| n as u32)
                    .unwrap_or(1),
                location
                    .and_then(|l| l.get("start_column").or_else(|| l.get("column")))
                    .and_then(Value::as_u64)
                    .map(|n| n as u32),
                offense
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

/// RuboCop `severity` to drep severity.
///
/// RuboCop's own ladder is
/// `refactor < convention < warning < error < fatal`, where the bottom two
/// rungs are stylistic preferences and the top two are defects. `warning`
/// falls to the catch-all Warning, which is also where an absent or
/// unrecognised severity lands; the default is safe because a middle rung
/// and a future name are both findings the project's own config asked for,
/// and Info would let them slip a gate that blocks on warnings.
fn rubocop_severity(severity: Option<&str>) -> Severity {
    match severity {
        Some("fatal") | Some("error") => Severity::Error,
        Some("convention") | Some("refactor") | Some("info") => Severity::Info,
        _ => Severity::Warning,
    }
}
