//! `drep lint-docs` - rule-based markdown checks. No LLM, no network.
//!
//! This command runs on every commit, so its startup path is load-bearing. It
//! touches [`crate::docs`] and [`crate::files`] and nothing else: no config
//! file is read, no provider chain is built, no response cache is opened. The
//! 1.x equivalent imported its workflows at module scope and paid 190 ms of
//! sqlalchemy and GitPython on every commit for a command that needs neither.
//!
//! Two exit rules, and they are independent:
//!
//! - A file the user named that drep could not analyze exits **2**, whether or
//!   not `--strict` was passed. "Report-only" governs *findings*; a file that
//!   went unread is not a finding, it is the absence of analysis, and that is
//!   the one thing drep never reports as clean.
//! - Findings exit **1** only under `--strict`. The default is report-only
//!   because the checks include whitespace and line length, and a gate that
//!   blocks a commit over a trailing space is a gate that gets switched off.

pub(crate) mod render;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::Result;
use clap::Args;

use crate::Exit;
use crate::analysis::findings::Finding;
use crate::analysis::result::FailureReason;
use crate::files;

#[derive(Debug, Args)]
pub struct LintDocsArgs {
    /// Markdown files or directories. Defaults to the current directory.
    #[arg(value_name = "PATH")]
    pub paths: Vec<PathBuf>,

    /// Exit non-zero when a check fires. Report-only by default.
    #[arg(long)]
    pub strict: bool,
}

/// Everything one `lint-docs` run produced.
pub struct LintOutcome {
    /// Every finding, sorted by file then by position.
    pub findings: Vec<Finding>,
    /// Files the user named that drep could not analyze.
    pub failures: BTreeMap<PathBuf, FailureReason>,
    /// The verdict. On the outcome rather than beside it, so the renderer and
    /// the process cannot disagree - the mistake `check` made and fixed.
    pub exit: Exit,
}

/// One `lint-docs` invocation.
///
/// Synchronous. Nothing here awaits: the command reads files and runs pure
/// checks, which is exactly what the module doc says it is for. An `async fn`
/// would advertise concurrency this command deliberately does not have.
pub fn run(args: &LintDocsArgs, root: &Path) -> Result<Exit> {
    let outcome = analyze(args, root);
    render::render(&outcome, args.strict)?;
    Ok(outcome.exit)
}

/// Resolve the paths, read each file, run the checks, and gate.
///
/// Split from [`run`] so a test asserts on the outcome without capturing
/// stdout, and so the rendering has nothing to decide.
fn analyze(args: &LintDocsArgs, root: &Path) -> LintOutcome {
    let mut failures: BTreeMap<PathBuf, FailureReason> = BTreeMap::new();
    let targets = resolve(&args.paths, root, &mut failures);

    let mut findings = Vec::new();
    for path in targets {
        match std::fs::read_to_string(&path) {
            Ok(content) => findings.extend(crate::docs::analyze(&path, &content)),
            Err(err) => {
                failures.insert(path, FailureReason::Unreadable(err.to_string()));
            }
        }
    }

    // The output contract is "reads top to bottom, file by file". Sorting by
    // file here rather than trusting the expander's order keeps that contract
    // owned by this function; `docs::analyze` has already ordered each file's
    // own findings, and a stable sort preserves that, so the position key is
    // not restated.
    findings.sort_by(|a, b| a.file_path.cmp(&b.file_path));

    let exit = gate(&findings, &failures, args.strict);
    LintOutcome {
        findings,
        failures,
        exit,
    }
}

/// Expand the arguments to markdown files, recording what the user named and
/// drep will not analyze.
///
/// The policy lives in [`files::expand_named`], shared with `check`, so the two
/// commands cannot disagree about what a named path that resolves to nothing
/// means.
fn resolve(
    paths: &[PathBuf],
    root: &Path,
    failures: &mut BTreeMap<PathBuf, FailureReason>,
) -> Vec<PathBuf> {
    let files::Expansion { targets, rejected } =
        files::expand_named(paths, root, files::is_markdown);
    for (path, why) in rejected {
        let reason = match why {
            files::Rejected::Missing => {
                FailureReason::Unreadable("no such file or directory".to_owned())
            }
            files::Rejected::Unanalyzable => {
                FailureReason::unsupported(&path, files::redirect_hint(&path))
            }
        };
        failures.insert(path, reason);
    }
    targets
}

/// Failures outrank findings, and findings only count under `--strict`.
///
/// The precedence matches `check`'s, deliberately: two commands in one binary
/// that disagree about what exit 2 means are two contracts a hook author has
/// to learn.
fn gate(findings: &[Finding], failures: &BTreeMap<PathBuf, FailureReason>, strict: bool) -> Exit {
    if !failures.is_empty() {
        return Exit::Unanalyzed;
    }
    if strict && !findings.is_empty() {
        return Exit::FoundIssues;
    }
    Exit::Clean
}

#[cfg(test)]
mod tests;
