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
use crate::languages::runner::uri::strip_file_uri;
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
        "sarif" => parse_sarif(spec, output),
        "ktlint" => parse_ktlint(spec, output),
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
            // Build the suggestion from `command` minus its last element, then
            // append `-w {path}`. For gofmt the removed element is `-l`.
            let suggest = format!(
                "Run `{base} -w {path}`",
                base = spec.command[..spec.command.len() - 1].join(" "),
            );
            Finding::deterministic(
                spec.name.to_owned(),
                Severity::Error,
                path.to_owned(),
                1,
                None,
                format!("{}: file is not formatted", spec.name),
                Some(suggest),
            )
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
        findings.push(Finding::deterministic(
            spec.name.to_owned(),
            Severity::Error,
            file.strip_prefix("./").unwrap_or(file).to_owned(),
            line_num,
            Some(col),
            message.to_owned(),
            None,
        ));
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
        findings.push(Finding::deterministic(
            code.to_owned(),
            Severity::Error,
            file.to_owned(),
            line_num,
            Some(col),
            message.to_owned(),
            None,
        ));
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
        // First primary span. With cargo's output exactly one span per
        // diagnostic is primary, and array order is preserved.
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

        findings.push(Finding::deterministic(
            kind,
            Severity::Error,
            file_path,
            line,
            column,
            message_text,
            None,
        ));
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

/// SARIF 2.1.0: `runs[].results[]`, the interchange format checkstyle emits.
///
/// Written against the spec rather than against checkstyle, because SARIF is
/// what the rest of the JVM linters converge on and a second one should need
/// no parser. Empty input is a clean run for the same reason it is under
/// `json`; anything else that will not parse is an error.
fn parse_sarif(spec: &ToolSpec, output: &str) -> Result<Vec<Finding>, ToolOutputError> {
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
        let results = run
            .get("results")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for result in results {
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
                .unwrap_or_default();
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
fn parse_ktlint(spec: &ToolSpec, output: &str) -> Result<Vec<Finding>, ToolOutputError> {
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
