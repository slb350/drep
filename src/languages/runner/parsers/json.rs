//! eslint- and ruff-shaped JSON, plus ktlint's near-miss of the same shape.

use serde_json::Value;

use crate::analysis::findings::{Finding, Severity};
use crate::languages::runner::ToolOutputError;
use crate::languages::spec::ToolSpec;

use super::json_kind_name;

/// Normalise ruff/eslint-shaped JSON into Findings.
pub(super) fn parse_json(
    spec: &ToolSpec,
    output: &str,
    root_name: &str,
) -> Result<Vec<Finding>, ToolOutputError> {
    // Empty input parses as `[]` - ruff and eslint print zero findings as
    // literally nothing on stdout, and that must not be a parse error.
    // Anything else that fails to parse is a real error: it means we are
    // not reading what we think we are.
    let trimmed = output.trim();
    let payload: Value = if trimmed.is_empty() {
        Value::Array(Vec::new())
    } else {
        serde_json::from_str(trimmed).map_err(|err| {
            ToolOutputError(format!("{} produced unparseable JSON: {err}", spec.name))
        })?
    };

    let entries = payload.as_array().ok_or_else(|| {
        ToolOutputError(format!(
            "{}: expected a JSON array, got {}",
            spec.name,
            json_kind_name(&payload)
        ))
    })?;

    let mut findings = Vec::new();
    for entry in entries {
        let obj = entry.as_object().ok_or_else(|| {
            ToolOutputError(format!("{}: expected objects in the array", spec.name))
        })?;

        // ruff shape: flat record with a `location`.
        if let Some(location) = obj.get("location") {
            let location = location.as_object();
            let row = location
                .and_then(|l| l.get("row"))
                .and_then(Value::as_u64)
                .map(|n| n as u32)
                .unwrap_or(1);
            let column = location
                .and_then(|l| l.get("column"))
                .and_then(Value::as_u64)
                .map(|n| n as u32);
            let file_path = obj
                .get("filename")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .unwrap_or_else(|| root_name.to_owned());
            let message = obj
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
            let kind = obj
                .get("code")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .unwrap_or_else(|| spec.name.to_owned());
            let suggestion = obj
                .get("fix")
                .and_then(|f| f.get("message"))
                .and_then(Value::as_str)
                .map(str::to_owned);
            findings.push(Finding::deterministic(
                kind,
                Severity::Error,
                file_path,
                row,
                column,
                message,
                suggestion,
            ));
            continue;
        }

        // eslint shape: one record per file with a nested `messages` array.
        let file_path = obj
            .get("filePath")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| root_name.to_owned());
        let messages = obj
            .get("messages")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for message in messages {
            let obj = message.as_object();
            let line = obj
                .and_then(|m| m.get("line"))
                .and_then(Value::as_u64)
                .map(|n| n as u32)
                .unwrap_or(1);
            let column = obj
                .and_then(|m| m.get("column"))
                .and_then(Value::as_u64)
                .map(|n| n as u32);
            let message_text = obj
                .and_then(|m| m.get("message"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
            let kind = obj
                .and_then(|m| m.get("ruleId"))
                .and_then(Value::as_str)
                .map(str::to_owned)
                .unwrap_or_else(|| spec.name.to_owned());
            findings.push(Finding::deterministic(
                kind,
                Severity::Error,
                file_path.clone(),
                line,
                column,
                message_text,
                None,
            ));
        }
    }
    Ok(findings)
}

/// ktlint's JSON reporter: one record per file with a nested `errors` array.
///
/// Shaped like eslint's but with different keys throughout - `file` for
/// `filePath`, `errors` for `messages`, `rule` for `ruleId` - so it gets its
/// own format rather than teaching `parse_json` to guess between them.
///
/// Why not ktlint's own `sarif` reporter, given `parse_sarif` exists: that
/// reporter files every result under a relative URI with
/// `uriBaseId: "%SRCROOT%"` bound to the process *home*, not the working
/// directory, so a finding's path resolves to nothing drep was asked to
/// check. The JSON reporter emits plain absolute paths instead.
pub(super) fn parse_ktlint(spec: &ToolSpec, output: &str) -> Result<Vec<Finding>, ToolOutputError> {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    let payload: Value = serde_json::from_str(trimmed).map_err(|err| {
        ToolOutputError(format!("{} produced unparseable JSON: {err}", spec.name))
    })?;
    let entries = payload.as_array().ok_or_else(|| {
        ToolOutputError(format!(
            "{}: expected a JSON array, got {}",
            spec.name,
            json_kind_name(&payload)
        ))
    })?;

    let mut findings = Vec::new();
    for entry in entries {
        let file_path = entry
            .get("file")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        let errors = entry
            .get("errors")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for error in errors {
            findings.push(Finding::deterministic(
                error
                    .get("rule")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .unwrap_or_else(|| spec.name.to_owned()),
                Severity::Error,
                file_path.clone(),
                error
                    .get("line")
                    .and_then(Value::as_u64)
                    .map(|n| n as u32)
                    .unwrap_or(1),
                error
                    .get("column")
                    .and_then(Value::as_u64)
                    .map(|n| n as u32),
                error
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
