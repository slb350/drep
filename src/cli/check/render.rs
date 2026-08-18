//! Render `CheckOutcome` to stdout.
//!
//! Two formats, one outcome:
//!
//! - **Text**: findings grouped by file, one per line, with the source
//!   prefix (`tool/` or `llm/`) and the suggestion on its own indented
//!   line when present. Followed by a "N file(s) could not be analyzed"
//!   block if any, and the exact `No issues found.\n` when nothing was
//!   produced and nothing failed.
//! - **JSON**: one object, pretty-printed, with `findings`, `unanalyzed`,
//!   and `exit`. The `unanalyzed` field is **always present** — even when
//!   empty — so a consumer can distinguish "no failures" from "this build
//!   of drep does not report them".
//!
//! The CLI does not `serde::Serialize` `Finding`; the wire shape is owned
//! here so the core type stays free of `serde` derives it doesn't need.

use std::collections::BTreeMap;
use std::io::Write;

use anyhow::Result;
use serde_json::json;

use crate::analysis::findings::Finding;
use crate::cli::OutputFormat;
use crate::cli::check::CheckOutcome;

/// Render the outcome to stdout in the requested format.
///
/// Text output goes to stdout; the CLI's own errors flow through `anyhow`
/// and end up on stderr via `main.rs`, so a clean run sees no stderr.
pub fn render(outcome: &CheckOutcome, format: OutputFormat) -> Result<()> {
    render_to(&mut std::io::stdout().lock(), outcome, format)
}

/// Render to an arbitrary sink, so a test can capture the bytes without a
/// subprocess.
pub fn render_to<W: Write>(
    out: &mut W,
    outcome: &CheckOutcome,
    format: OutputFormat,
) -> Result<()> {
    match format {
        OutputFormat::Text => render_text(out, outcome),
        OutputFormat::Json => render_json(out, outcome),
    }
}

/// Render the text format. Findings first, grouped by file, then the
/// "unanalyzed" block, then the trailing clean message.
fn render_text<W: Write>(out: &mut W, outcome: &CheckOutcome) -> Result<()> {
    let mut all_findings: Vec<(&'static str, &Finding)> = Vec::new();
    for f in &outcome.tool_findings {
        all_findings.push(("tool", f));
    }
    for f in &outcome.llm_findings {
        all_findings.push(("llm", f));
    }

    // Findings written in (file, line) order so the output is stable across
    // runs of the same diff. A `BTreeMap` keyed on a per-file, per-line
    // string beats pulling a sort lib for what is at most a few hundred
    // entries on a real commit.
    let mut by_position: BTreeMap<(String, u32, usize), String> = BTreeMap::new();
    for (idx, (source, f)) in all_findings.iter().enumerate() {
        let line = format_finding_line(source, f);
        by_position.insert((f.file_path.clone(), f.line, idx), line);
    }
    let mut suggestion_map: BTreeMap<(String, u32, usize), String> = BTreeMap::new();
    for (idx, (_source, f)) in all_findings.iter().enumerate() {
        if let Some(suggestion) = &f.suggestion {
            let line = format!("    suggestion: {suggestion}");
            suggestion_map.insert((f.file_path.clone(), f.line, idx), line);
        }
    }
    for line in by_position.values() {
        writeln!(out, "{line}")?;
    }
    for key in by_position.keys() {
        if let Some(line) = suggestion_map.remove(key) {
            writeln!(out, "{line}")?;
        }
    }

    if !outcome.failures.is_empty() {
        writeln!(out)?;
        writeln!(
            out,
            "{} file(s) could not be analyzed:",
            outcome.failures.len()
        )?;
        for (path, reason) in &outcome.failures {
            writeln!(out, "  {}: {}", path.display(), reason.one_line())?;
        }
    }

    if all_findings.is_empty() && outcome.failures.is_empty() {
        writeln!(out, "No issues found.")?;
    }

    Ok(())
}

/// One line of text output for a finding.
///
/// The `tool/` or `llm/` prefix is the source; the path is the file; the
/// rest is position, severity, kind, and message. The exact format is
/// pinned by the spec's example and by the text-output acceptance test.
fn format_finding_line(source: &'static str, f: &Finding) -> String {
    let severity = f.severity.as_str();
    let kind = &f.kind;
    let message = &f.message;
    let path = &f.file_path;
    let line = f.line;
    let column = f.column.map(|c| format!(":{c}")).unwrap_or_default();
    format!("{path}:{line}{column}: {severity} [{source}/{kind}] {message}")
}

/// Render the JSON format. One object, pretty-printed, on stdout.
fn render_json<W: Write>(out: &mut W, outcome: &CheckOutcome) -> Result<()> {
    let findings: Vec<_> = outcome
        .tool_findings
        .iter()
        .map(|f| finding_json("tool", f))
        .chain(outcome.llm_findings.iter().map(|f| finding_json("llm", f)))
        .collect();

    let unanalyzed: Vec<_> = outcome
        .failures
        .iter()
        .map(|(path, reason)| {
            json!({
                "file": path.to_string_lossy(),
                "reason": reason.one_line(),
            })
        })
        .collect();

    // The gate's verdict, passed in - never recomputed. A second exit
    // computation here ignored `--fail-on`, so a run with an LLM finding and no
    // `--fail-on` exited 0 while the JSON said 1.
    let exit = outcome.exit.code();
    let payload = json!({
        "findings": findings,
        "unanalyzed": unanalyzed,
        "exit": exit,
    });
    serde_json::to_writer_pretty(&mut *out, &payload)?;
    writeln!(out)?;
    Ok(())
}

/// One entry in the JSON `findings` array.
fn finding_json(source: &'static str, f: &Finding) -> serde_json::Value {
    let mut obj = serde_json::Map::new();
    obj.insert("source".to_owned(), json!(source));
    obj.insert("kind".to_owned(), json!(f.kind));
    obj.insert("severity".to_owned(), json!(f.severity.as_str()));
    obj.insert("file".to_owned(), json!(f.file_path));
    obj.insert("line".to_owned(), json!(f.line));
    if let Some(column) = f.column {
        obj.insert("column".to_owned(), json!(column));
    }
    obj.insert("message".to_owned(), json!(f.message));
    obj.insert("suggestion".to_owned(), json!(f.suggestion));
    serde_json::Value::Object(obj)
}
