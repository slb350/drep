//! Render a [`LintOutcome`] to stdout.
//!
//! Text only. `lint-docs` has no `--format json`: it is a hook-facing command
//! whose consumer is a human reading a terminal, and adding a wire format
//! before anything consumes one means committing to a shape nobody has tested
//! against.
//!
//! The finding line and the failure block come from `cli::render`, shared with
//! `check`. The one deliberate difference is the source prefix: `check` tags
//! each line `tool/` or `llm/` because two layers write into one list and they
//! gate differently, and passing `None` here is what says "one source".

use std::io::Write;

use anyhow::Result;

use crate::cli::lint_docs::LintOutcome;
use crate::cli::render::{finding_line, write_failures, write_suggestion};

/// Render to stdout.
pub fn render(outcome: &LintOutcome, strict: bool) -> Result<()> {
    render_to(&mut std::io::stdout().lock(), outcome, strict)
}

/// Render to an arbitrary sink, so a test can capture the bytes without a
/// subprocess.
pub fn render_to<W: Write>(out: &mut W, outcome: &LintOutcome, strict: bool) -> Result<()> {
    for finding in &outcome.findings {
        writeln!(out, "{}", finding_line(None, finding))?;
        write_suggestion(out, finding)?;
    }
    write_failures(out, &outcome.failures, !outcome.findings.is_empty())?;

    if outcome.findings.is_empty() && outcome.failures.is_empty() {
        writeln!(out, "No issues found.")?;
        return Ok(());
    }

    // The report-only footer exists because the exit code is otherwise the
    // only signal, and it is zero. Without this line a hook that prints forty
    // findings and lets the commit through looks broken.
    if !outcome.findings.is_empty() {
        writeln!(out)?;
        let count = outcome.findings.len();
        if strict {
            writeln!(out, "{count} issue(s) found.")?;
        } else {
            writeln!(
                out,
                "{count} issue(s) found (report only; pass --strict to fail)."
            )?;
        }
    }
    Ok(())
}
