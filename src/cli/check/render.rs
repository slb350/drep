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
    // One pass, one map. Each entry carries its finding line and that
    // finding's suggestion, so the suggestion is written immediately after the
    // line it belongs to. Printing every finding first and every suggestion
    // afterwards - as this did - detaches them: with two findings, the first
    // suggestion appears below the second finding and reads as if it belonged
    // to it.
    let mut by_position: BTreeMap<(String, u32, usize), (String, Option<String>)> = BTreeMap::new();
    for (idx, (source, f)) in tagged(outcome).enumerate() {
        by_position.insert(
            (f.file_path.clone(), f.line, idx),
            (
                format_finding_line(source, f),
                f.suggestion
                    .as_ref()
                    .map(|s| format!("    suggestion: {s}")),
            ),
        );
    }

    let clean = by_position.is_empty() && outcome.failures.is_empty();
    for (line, suggestion) in by_position.values() {
        writeln!(out, "{line}")?;
        if let Some(suggestion) = suggestion {
            writeln!(out, "{suggestion}")?;
        }
    }

    if !outcome.failures.is_empty() {
        // Blank separator between the findings block and the failure block.
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

    if clean {
        writeln!(out, "No issues found.")?;
    }
    Ok(())
}

/// Every finding paired with the layer that produced it.
///
/// One statement of the source tagging, consumed by both renderers. It was
/// spelled as two push loops in the text path and a `.chain()` in the JSON
/// path, which is two places for "which layer is this" to drift.
fn tagged(outcome: &CheckOutcome) -> impl Iterator<Item = (&'static str, &Finding)> {
    outcome
        .tool_findings
        .iter()
        .map(|f| ("tool", f))
        .chain(outcome.llm_findings.iter().map(|f| ("llm", f)))
}

/// One line of text output for a finding.
///
/// The `tool/` or `llm/` prefix is the source; the path is the file; the rest
/// is position, severity, kind, and message. The exact format is pinned by the
/// text-output acceptance test.
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
    let findings: Vec<_> = tagged(outcome)
        .map(|(source, f)| finding_json(source, f))
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
