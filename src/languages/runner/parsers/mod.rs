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
//! Note the deliberate asymmetry in strictness. `position` and `tsc` are
//! line-oriented and *skip* lines they do not recognise, because Go interleaves
//! `# example.com/pkg` package headers among its diagnostics. The JSON-shaped
//! parsers do not get that latitude: input we cannot parse means we do not know
//! whether the file is clean, and guessing "clean" is the failure this module
//! exists to prevent.

use serde_json::Value;

use crate::analysis::findings::Finding;
use crate::languages::runner::ToolOutputError;
use crate::languages::spec::ToolSpec;

mod cargo;
mod json;
mod lines;
mod position;
mod sarif;

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
        "tsc" => Ok(position::parse_tsc(spec, output)),
        "cargo" => cargo::parse_cargo(spec, output),
        "sarif" => sarif::parse_sarif(spec, output),
        "ktlint" => json::parse_ktlint(spec, output),
        other => Err(ToolOutputError(format!(
            "{}: no parser for output format {other:?}",
            spec.name,
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
