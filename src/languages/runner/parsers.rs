//! Turn each tool's own output shape into `Finding`s.
//!
//! Everything tool-specific about *reading* diagnostics lives here, so nothing
//! downstream knows which tool produced a finding. The split from `mod.rs` is
//! by concern: that module decides whether a tool runs, this one reads what it
//! said.
//!
//! Note the deliberate asymmetry in strictness. `position` and `tsc` are
//! line-oriented and *skip* lines they do not recognise, because Go interleaves
//! `# example.com/pkg` package headers among its diagnostics. `json` and
//! `cargo` do not get that latitude: input we cannot parse means we do not know
//! whether the file is clean, and guessing "clean" is the failure this module
//! exists to prevent.

use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

use crate::analysis::findings::{Finding, Severity};
use crate::languages::runner::ToolOutputError;
use crate::languages::spec::ToolSpec;

/// `[vet: ]./path/to/file.go:12:6: message` - the compiler-style position
/// that `go vet` and most Go tooling emit.
///
/// The optional `vet: ` prefix and the `^` anchor matter: Go interleaves
/// `# example.com/pkg` package headers, and we skip those by *not* matching.
static POSITION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:vet:\s*)?(?P<file>[^\s:][^:]*):(?P<line>\d+):(?P<col>\d+):\s*(?P<message>.+)$")
        .expect("POSITION regex compiles")
});

/// `src/app.ts(14,22): error TS2345: message` - the tsc shape.
static TSC: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^(?P<file>.+?)\((?P<line>\d+),(?P<col>\d+)\):\s*(?:error|warning)\s+(?P<code>TS\d+):\s*(?P<message>.+)$",
    )
    .expect("TSC regex compiles")
});

/// Convert a tool's diagnostics into Findings.
///
/// Every tool's own shape is normalised here, so nothing downstream of this
/// module knows which tool produced a finding.
///
/// `root_name` is the fallback file path when a JSON entry omits one.
/// `run_tool` passes the first arg path, but tests pass an explicit value.
pub fn parse_output(
    spec: &ToolSpec,
    output: &str,
    root_name: &str,
) -> Result<Vec<Finding>, ToolOutputError> {
    match spec.output_format {
        "lines" => Ok(parse_lines(spec, output)),
        "json" => parse_json(spec, output, root_name),
        "position" => Ok(parse_positions(spec, output)),
        "tsc" => Ok(parse_tsc(spec, output)),
        "cargo" => parse_cargo(spec, output),
        other => Err(ToolOutputError(format!(
            "{}: no parser for output format {other:?}",
            spec.name,
        ))),
    }
}

/// `gofmt -l` prints one path per line: each non-blank line is a file that
/// needs formatting.
fn parse_lines(spec: &ToolSpec, output: &str) -> Vec<Finding> {
    output
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let path = line.trim();
            // The Python reference builds the suggestion from `command`
            // minus its last element, then `-w {path}`. For gofmt the last
            // element is `-l`, so we get `gofmt -w {path}`.
            let suggest = format!(
                "Run `{base} -w {path}`",
                base = spec.command[..spec.command.len() - 1].join(" "),
            );
            Finding {
                kind: spec.name.to_owned(),
                severity: Severity::Error,
                file_path: path.to_owned(),
                line: 1,
                column: None,
                message: format!("{}: file is not formatted", spec.name),
                suggestion: Some(suggest),
            }
        })
        .collect()
}

/// `file:line:col: message`, skipping the package headers Go interleaves.
fn parse_positions(spec: &ToolSpec, output: &str) -> Vec<Finding> {
    let mut findings = Vec::new();
    for line in output.lines() {
        let Some(caps) = POSITION.captures(line.trim()) else {
            // `# example.com/pkg` headers and blank lines are not diagnostics.
            continue;
        };
        let file = caps.name("file").map(|m| m.as_str()).unwrap_or("");
        let line_num = caps
            .name("line")
            .and_then(|m| m.as_str().parse::<u32>().ok())
            .unwrap_or(1);
        let col = caps
            .name("col")
            .and_then(|m| m.as_str().parse::<u32>().ok())
            .unwrap_or(1);
        let message = caps
            .name("message")
            .map(|m| m.as_str().trim())
            .unwrap_or("");
        findings.push(Finding {
            kind: spec.name.to_owned(),
            severity: Severity::Error,
            file_path: file.strip_prefix("./").unwrap_or(file).to_owned(),
            line: line_num,
            column: Some(col),
            message: message.to_owned(),
            suggestion: None,
        });
    }
    findings
}

/// `file(line,col): error TS1234: message`.
fn parse_tsc(spec: &ToolSpec, output: &str) -> Vec<Finding> {
    let mut findings = Vec::new();
    for line in output.lines() {
        let Some(caps) = TSC.captures(line.trim()) else {
            continue;
        };
        let file = caps.name("file").map(|m| m.as_str()).unwrap_or("");
        let line_num = caps
            .name("line")
            .and_then(|m| m.as_str().parse::<u32>().ok())
            .unwrap_or(1);
        let col = caps
            .name("col")
            .and_then(|m| m.as_str().parse::<u32>().ok())
            .unwrap_or(1);
        let code = caps.name("code").map(|m| m.as_str()).unwrap_or(spec.name);
        let message = caps
            .name("message")
            .map(|m| m.as_str().trim())
            .unwrap_or("");
        findings.push(Finding {
            kind: code.to_owned(),
            severity: Severity::Error,
            file_path: file.to_owned(),
            line: line_num,
            column: Some(col),
            message: message.to_owned(),
            suggestion: None,
        });
    }
    findings
}

/// cargo's newline-delimited JSON: one object per event, not one array.
///
/// Only `compiler-message` events are diagnostics; the rest are build
/// progress. A line that is not JSON at all is an **error** rather than a
/// skip, since that means we are not reading what we think we are.
fn parse_cargo(spec: &ToolSpec, output: &str) -> Result<Vec<Finding>, ToolOutputError> {
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

        let message = event.get("message").cloned().unwrap_or(Value::Null);
        // First primary span. With cargo's emit, exactly one span per
        // diagnostic is primary, but ordering in the array is preserved, so
        // `find` is equivalent to the Python `[s for s in spans if
        // s.is_primary][0]`.
        let primary = message
            .get("spans")
            .and_then(Value::as_array)
            .and_then(|spans| {
                spans
                    .iter()
                    .find(|s| s.get("is_primary") == Some(&Value::Bool(true)))
            })
            .cloned()
            .unwrap_or(Value::Null);

        let kind = message
            .get("code")
            .and_then(|c| c.get("code"))
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| spec.name.to_owned());

        let file_path = primary
            .get("file_name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        let line = primary
            .get("line_start")
            .and_then(Value::as_u64)
            .map(|n| n as u32)
            .unwrap_or(1);
        let column = primary
            .get("column_start")
            .and_then(Value::as_u64)
            .map(|n| n as u32);
        let message_text = message
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();

        findings.push(Finding {
            kind,
            severity: Severity::Error,
            file_path,
            line,
            column,
            message: message_text,
            suggestion: None,
        });
    }
    Ok(findings)
}

/// Normalise ruff/eslint-shaped JSON into Findings.
fn parse_json(
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
            findings.push(Finding {
                kind,
                severity: Severity::Error,
                file_path,
                line: row,
                column,
                message,
                suggestion,
            });
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
            findings.push(Finding {
                kind,
                severity: Severity::Error,
                file_path: file_path.clone(),
                line,
                column,
                message: message_text,
                suggestion: None,
            });
        }
    }
    Ok(findings)
}

/// Human-readable name for a JSON value's outer kind, for error messages.
fn json_kind_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}
