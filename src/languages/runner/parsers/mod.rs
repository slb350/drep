//! Turn each tool's own output shape into `Finding`s.
//!
//! Everything tool-specific about *reading* diagnostics lives here, so nothing
//! downstream knows which tool produced a finding. The split from `mod.rs` is
//! by concern: that module decides whether a tool runs, this one reads what it
//! said.
//!
//! One submodule per output *shape*, not per tool. SARIF is the clearest case:
//! five of the checkers drep ships emit it, so they share `sarif.rs` and a
//! sixth SARIF producer costs a table entry rather than a parser. A tool earns
//! its own module only when its shape is genuinely its own.
//!
//! Note the deliberate asymmetry in strictness. `position`, `tsc` and
//! `msbuild` are line-oriented and *skip* lines they do not recognise,
//! because Go interleaves `# example.com/pkg` package headers and MSBuild
//! restore and build chatter among their diagnostics. The JSON-shaped
//! parsers do not get that latitude: input we cannot parse means we do not
//! know whether the file is clean, and guessing "clean" is the failure this
//! module exists to prevent.

use serde_json::{Map, Value};

use crate::analysis::findings::Finding;
use crate::languages::runner::ToolOutputError;
use crate::languages::spec::ToolSpec;

mod cargo;
mod credo;
mod json;
mod lines;
mod phpcs;
mod position;
mod rubocop;
mod sarif;
mod shellcheck;
mod sqlfluff;

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
        "lines" => Ok(lines::parse_lines(spec, output)),
        "json" => json::parse_json(spec, output, root_name),
        "position" => Ok(position::parse_positions(spec, output)),
        "tsc" => Ok(position::parse_tsc(output)),
        "cargo" => cargo::parse_cargo(spec, output),
        "sarif" => sarif::parse_sarif(spec, output),
        "ktlint" => json::parse_ktlint(spec, output),
        "shellcheck" => shellcheck::parse_shellcheck(spec, output),
        "rubocop" => rubocop::parse_rubocop(spec, output),
        "phpcs" => phpcs::parse_phpcs(spec, output),
        "credo" => credo::parse_credo(spec, output),
        "sqlfluff" => sqlfluff::parse_sqlfluff(spec, output),
        "msbuild" => Ok(position::parse_msbuild(output)),
        other => Err(ToolOutputError(format!(
            "{}: no parser for output format {other:?}",
            spec.name,
        ))),
    }
}

/// Parse a tool's stdout as one JSON document, treating empty output as a
/// clean run.
///
/// The JSON-shaped checkers print zero findings as literally nothing on
/// stdout, so empty or whitespace-only input is `Ok(None)` rather than a
/// parse error. Anything else that fails to parse is a real error: it means
/// we are not reading what we think we are. SARIF keeps its own preamble
/// because its error names the format: "unparseable SARIF" and "unparseable
/// JSON" are different failures.
pub(super) fn json_payload(
    spec: &ToolSpec,
    output: &str,
) -> Result<Option<Value>, ToolOutputError> {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    serde_json::from_str(trimmed)
        .map(Some)
        .map_err(|err| ToolOutputError(format!("{} produced unparseable JSON: {err}", spec.name)))
}

/// The payload as a JSON array, or an error naming what arrived instead.
pub(super) fn expect_array<'a>(
    spec: &ToolSpec,
    payload: &'a Value,
) -> Result<&'a Vec<Value>, ToolOutputError> {
    payload.as_array().ok_or_else(|| {
        ToolOutputError(format!(
            "{}: expected a JSON array, got {}",
            spec.name,
            json_kind_name(payload)
        ))
    })
}

/// The array stored under `key`, or an error naming what arrived instead.
///
/// A missing key and a wrong-typed key get distinct messages because they
/// mean different things: the first is not the tool's JSON at all, the second
/// is a newer schema reading the same field differently.
pub(super) fn expect_keyed_array<'a>(
    spec: &ToolSpec,
    payload: &'a Value,
    key: &str,
) -> Result<&'a Vec<Value>, ToolOutputError> {
    match payload.get(key) {
        Some(value) => value.as_array().ok_or_else(|| {
            ToolOutputError(format!(
                "{}: expected {key} to be an array, got {}",
                spec.name,
                json_kind_name(value)
            ))
        }),
        None => Err(ToolOutputError(format!(
            "{}: expected an {key} array, got {}",
            spec.name,
            json_kind_name(payload)
        ))),
    }
}

/// The object stored under `key`, or an error naming what arrived instead.
///
/// The missing-key versus wrong-typed-key distinction is the same as
/// `expect_keyed_array`; the object form exists for phpcs, the one checker
/// whose payload maps by path rather than listing.
pub(super) fn expect_keyed_object<'a>(
    spec: &ToolSpec,
    payload: &'a Value,
    key: &str,
) -> Result<&'a Map<String, Value>, ToolOutputError> {
    match payload.get(key) {
        Some(value) => value.as_object().ok_or_else(|| {
            ToolOutputError(format!(
                "{}: expected {key} to be an object, got {}",
                spec.name,
                json_kind_name(value)
            ))
        }),
        None => Err(ToolOutputError(format!(
            "{}: expected a {key} object, got {}",
            spec.name,
            json_kind_name(payload)
        ))),
    }
}

/// Human-readable name for a JSON value's outer kind, for error messages.
pub(super) fn json_kind_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}
