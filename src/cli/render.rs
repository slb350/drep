//! Output shared by the commands that report findings.
//!
//! `check` and `lint-docs` print the same two things in the same way: a
//! finding line, and the "could not be analyzed" block that carries the exit-2
//! contract. They were transcribed copies, so `lint-docs` inherited a fixed
//! bug by luck rather than by construction: the blank line above the failure
//! block is emitted only when a findings block precedes it, because emitting it
//! unconditionally opened every clean-but-unanalyzed run with a stray empty
//! line.
//!
//! What each command still owns is its own format's shape - `check`'s JSON
//! wire types, `lint-docs`' report-only footer - and the one deliberate
//! difference in the finding line, which is passed as an argument rather than
//! copied.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;

use anyhow::Result;

use crate::analysis::findings::Finding;
use crate::analysis::result::FailureReason;

/// One line of text output for a finding.
///
/// `source` is the layer that produced it (`"tool"`, `"llm"`), rendered as a
/// prefix inside the brackets. `check` passes one because two layers write into
/// one list and they gate differently; `lint-docs` passes `None`, because it
/// has a single source and a constant tag on every line says nothing.
pub fn finding_line(source: Option<&str>, f: &Finding) -> String {
    let column = f.column.map(|c| format!(":{c}")).unwrap_or_default();
    let tag = match source {
        Some(source) => format!("{source}/{}", f.kind),
        None => f.kind.clone(),
    };
    format!(
        "{}:{}{}: {} [{}] {}",
        f.file_path,
        f.line,
        column,
        f.severity.as_str(),
        tag,
        f.message
    )
}

/// The suggestion line that follows a finding, when it has one.
///
/// Written immediately after the finding it belongs to. Printing every finding
/// first and every suggestion afterwards detaches them: with two findings, the
/// first suggestion appears below the second finding and reads as if it
/// belonged to it.
pub fn write_suggestion<W: Write>(out: &mut W, f: &Finding) -> Result<()> {
    if let Some(suggestion) = &f.suggestion {
        writeln!(out, "    suggestion: {suggestion}")?;
    }
    Ok(())
}

/// The "N file(s) could not be analyzed" block.
///
/// `findings_above` decides the separating blank line - see the module doc.
pub fn write_failures<W: Write>(
    out: &mut W,
    failures: &BTreeMap<PathBuf, FailureReason>,
    findings_above: bool,
) -> Result<()> {
    if failures.is_empty() {
        return Ok(());
    }
    if findings_above {
        writeln!(out)?;
    }
    writeln!(out, "{} file(s) could not be analyzed:", failures.len())?;
    for (path, reason) in failures {
        writeln!(out, "  {}: {}", path.display(), reason.one_line())?;
    }
    Ok(())
}
