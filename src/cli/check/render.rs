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
//!   `providers`, and `exit`. The `unanalyzed` and `providers` fields are
//!   **always present** — even when empty — so a consumer can distinguish "no
//!   failures" from "this build of drep does not report them". Each
//!   `unanalyzed` entry carries a machine-readable `kind`, plus `status` for
//!   HTTP failures or `backend_kind` for process-backend failures, beside the
//!   human `reason`.
//!
//! The failover report is silent on the happy path. A run served entirely by
//! the head of the chain prints nothing about providers: that is the expected
//! case, and a line saying so on every commit is noise a user learns to skip.
//! A run that fell through to a second provider prints who served what, because
//! a silent switch from a local model to a paid API is a cost surprise.
//!
//! The CLI does not `serde::Serialize` `Finding`; the wire shape is owned
//! here so the core type stays free of `serde` derives it doesn't need.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

use anyhow::Result;
use serde_json::json;

use crate::analysis::findings::Finding;
use crate::analysis::result::{FailureReason, ProviderFailure};
use crate::cli::OutputFormat;
use crate::cli::check::{CheckOutcome, ProviderUse};
use crate::cli::render::{finding_line, write_failures};

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
    let mut by_position: BTreeMap<(&str, u32, usize), (String, Option<String>)> = BTreeMap::new();
    for (idx, (source, f)) in tagged(outcome).enumerate() {
        by_position.insert(
            (f.file_path.as_str(), f.line, idx),
            (
                finding_line(Some(source), f),
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

    write_failures(out, &outcome.failures, !by_position.is_empty())?;

    write_provider_block(out, &outcome.provider_uses)?;

    if clean {
        writeln!(out, "No issues found.")?;
    }
    Ok(())
}

/// The failover report (see the module doc for when it prints at all).
///
/// The whole list is printed once it triggers, not just the fallback's line:
/// "12 files went to the paid endpoint" is only actionable next to "4 went to
/// the local one".
fn write_provider_block<W: Write>(out: &mut W, uses: &[ProviderUse]) -> Result<()> {
    if !uses.iter().any(|use_| use_.index > 0) {
        return Ok(());
    }
    writeln!(out)?;
    writeln!(out, "LLM providers used (the chain fell through):")?;
    for use_ in uses {
        writeln!(
            out,
            "  {}. {} at {}: {} file(s)",
            use_.index + 1,
            use_.model,
            use_.location,
            use_.files
        )?;
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

/// Render the JSON format. One object, pretty-printed, on stdout.
fn render_json<W: Write>(out: &mut W, outcome: &CheckOutcome) -> Result<()> {
    let findings: Vec<_> = tagged(outcome)
        .map(|(source, f)| finding_json(source, f))
        .collect();

    let unanalyzed: Vec<_> = outcome
        .failures
        .iter()
        .map(|(path, reason)| unanalyzed_json(path, reason))
        .collect();

    let providers: Vec<_> = outcome
        .provider_uses
        .iter()
        .map(provider_use_json)
        .collect();

    // The gate's verdict, passed in - never recomputed. A second exit
    // computation here ignored `--fail-on`, so a run with an LLM finding and no
    // `--fail-on` exited 0 while the JSON said 1.
    let exit = outcome.exit.code();
    let payload = json!({
        "findings": findings,
        "unanalyzed": unanalyzed,
        "providers": providers,
        "exit": exit,
    });
    serde_json::to_writer_pretty(&mut *out, &payload)?;
    writeln!(out)?;
    Ok(())
}

/// The stable machine tag for a failure, as it appears in JSON.
///
/// Here rather than on `FailureReason` because this module already owns the
/// wire shape - it declines to `Serialize` `Finding` for the same reason, so
/// that the core types stay free of a JSON contract they do not otherwise
/// need. A caller in the crate branches on the enum, which is exhaustive and
/// compiler-checked; only a caller reading the JSON needs a string.
///
/// Deliberately not derived from the variant names: a rename inside the crate
/// must not silently change the published tag.
fn failure_kind(reason: &FailureReason) -> &'static str {
    match reason {
        FailureReason::Transport { .. } => "transport",
        FailureReason::Backend { .. } => "backend",
        FailureReason::Unparseable(_) => "unparseable",
        FailureReason::ModelStopped { .. } => "model_stopped",
        FailureReason::Truncated => "truncated",
        FailureReason::MalformedFinding(_) => "malformed_finding",
        FailureReason::ToolUnavailable { .. } => "tool_unavailable",
        FailureReason::FileTooLarge { .. } => "file_too_large",
        FailureReason::PayloadTooLarge { .. } => "payload_too_large",
        FailureReason::Unreadable(_) => "unreadable",
        FailureReason::Unsupported { .. } => "unsupported",
        FailureReason::ChainFailed(_) => "chain_failed",
    }
}

/// One entry in the JSON `unanalyzed` array.
///
/// Three keys, deliberately: `kind` is the stable machine tag, `reason` is the
/// same human line the text format prints, and `status` appears only when the
/// failure carried an HTTP code. The reason used to be the *only* key, which
/// meant a consumer wanting to tell a rate limit from a dead endpoint had to
/// pattern-match English — and phase 5c's failover has to make exactly that
/// distinction, because a 429 should fail over and a 401 must not.
///
/// `status` is omitted rather than emitted as `null` so its presence is itself
/// the signal that the failure was an HTTP one.
fn unanalyzed_json(path: &Path, reason: &FailureReason) -> serde_json::Value {
    let mut obj = serde_json::Map::new();
    obj.insert("file".to_owned(), json!(path.to_string_lossy()));
    if let FailureReason::ChainFailed(failures) = reason {
        // The per-provider detail as structure, not prose. `status` on the
        // outer object stays absent here (`FailureReason::status` returns
        // `None` for a chain failure): there is one status per provider, and
        // flattening them to one number would tell a consumer the last
        // provider's 401 was the whole story.
        obj.insert(
            "providers".to_owned(),
            json!(
                failures
                    .iter()
                    .map(provider_failure_json)
                    .collect::<Vec<_>>()
            ),
        );
    }
    insert_reason(&mut obj, reason);
    serde_json::Value::Object(obj)
}

/// One entry in an `unanalyzed` record's `providers` array.
///
/// `provider` is one-based, matching every other place drep numbers the chain
/// (`doctor`'s listing, the text failure line, the chain's own error
/// messages). A consumer correlating this with the config file counts tables
/// from one, not from zero.
fn provider_failure_json(failure: &ProviderFailure) -> serde_json::Value {
    let mut obj = serde_json::Map::new();
    obj.insert("provider".to_owned(), json!(failure.provider + 1));
    obj.insert("model".to_owned(), json!(failure.model));
    obj.insert("skipped".to_owned(), json!(failure.skipped));
    insert_reason(&mut obj, &failure.reason);
    serde_json::Value::Object(obj)
}

/// Write the `kind` / optional `status` / `reason` triple every failure record
/// carries, at either nesting level.
///
/// One function because the "omit `status` rather than emit `null`" rule is one
/// decision: its presence is itself the signal that the failure was an HTTP
/// one, and two copies of that would let the convention hold at one level and
/// not the other.
fn insert_reason(obj: &mut serde_json::Map<String, serde_json::Value>, reason: &FailureReason) {
    obj.insert("kind".to_owned(), json!(failure_kind(reason)));
    if let FailureReason::Backend { kind, .. } = reason {
        obj.insert("backend_kind".to_owned(), json!(kind.as_str()));
    }
    if let Some(status) = reason.status() {
        obj.insert("status".to_owned(), json!(status));
    }
    obj.insert("reason".to_owned(), json!(reason.one_line()));
}

/// One entry in the JSON `providers` array: who answered, and for how many
/// files.
fn provider_use_json(use_: &ProviderUse) -> serde_json::Value {
    json!({
        "provider": use_.index + 1,
        "model": use_.model,
        // Keep the published JSON field for compatibility; internally a
        // process backend has a location rather than an HTTP endpoint.
        "endpoint": use_.location,
        "files": use_.files,
    })
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
