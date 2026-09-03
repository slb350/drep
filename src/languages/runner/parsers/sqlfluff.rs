//! sqlfluff's `lint --format json`: a flat array of per-file violation lists.

use serde_json::Value;

use crate::analysis::findings::{Finding, Severity};
use crate::languages::runner::ToolOutputError;
use crate::languages::spec::ToolSpec;

use super::{expect_array, json_payload};

/// `sqlfluff lint --format json`: `[{filepath, violations: [...]}]`.
pub(super) fn parse_sqlfluff(
    spec: &ToolSpec,
    output: &str,
) -> Result<Vec<Finding>, ToolOutputError> {
    let Some(payload) = json_payload(spec, output)? else {
        return Ok(Vec::new());
    };
    let entries = expect_array(spec, &payload)?;

    let mut findings = Vec::new();
    for entry in entries {
        let file_path = entry
            .get("filepath")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        for violation in entry
            .get("violations")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            findings.push(Finding::deterministic(
                violation
                    .get("code")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .unwrap_or_else(|| spec.name.to_owned()),
                sqlfluff_severity(violation.get("warning")),
                file_path.clone(),
                violation
                    .get("start_line_no")
                    .and_then(Value::as_u64)
                    .map(|n| n as u32)
                    .unwrap_or(1),
                violation
                    .get("start_line_pos")
                    .and_then(Value::as_u64)
                    .map(|n| n as u32),
                violation
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
                None,
            ));
        }
    }
    Ok(findings)
}

/// sqlfluff's `warning` boolean to drep severity.
///
/// sqlfluff emits no severity string: every rule violation is an error
/// unless the project explicitly downgraded the rule in its config, which
/// is the only case `warning: true` appears. `warning: false` falls to the
/// catch-all Error, which is also where an absent field lands; the
/// default is safe because a violation that never said it was downgraded
/// is an error.
fn sqlfluff_severity(warning: Option<&Value>) -> Severity {
    match warning {
        Some(Value::Bool(true)) => Severity::Warning,
        _ => Severity::Error,
    }
}
