//! `drep lint-docs` - rule-based markdown checks. No LLM, no network.
//!
//! This command runs on every commit, so its startup path is load-bearing. It
//! touches [`crate::docs`], [`crate::files`] and - in `--staged` mode only -
//! [`crate::diff`], and nothing else: no config file is read, no provider
//! chain is built, and no response cache is opened.
//!
//! Two exit rules, and they are independent:
//!
//! - A file the user named that drep could not analyze exits **2**, whether or
//!   not `--strict` was passed. "Report-only" governs *findings*; a file that
//!   went unread is not a finding, it is the absence of analysis, and that is
//!   the one thing drep never reports as clean.
//! - Findings exit **1** only at or above `--fail-on`, which is unset by
//!   default. The default is report-only because the checks include whitespace
//!   and line length, and a gate that blocks a commit over a trailing space is
//!   a gate that gets switched off. `--strict` is the shorthand for
//!   `--fail-on info`: one mechanism, asked at two thresholds.

pub(crate) mod render;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::Result;
use clap::{ArgGroup, Args};

use crate::Exit;
use crate::analysis::findings::{self, Finding, Severity};
use crate::analysis::result::FailureReason;
use crate::cli::severity_parser;
use crate::files;

#[derive(Debug, Args)]
// Same shape as `check`'s input group, and stated once for the same reason:
// paired `conflicts_with` attributes say it from each side and can drift.
// Deliberately not `required`: bare `drep lint-docs` means "this tree".
#[command(group(ArgGroup::new("docs-input").args(["paths", "staged"]).multiple(false)))]
pub struct LintDocsArgs {
    /// Markdown files or directories. Defaults to the current directory.
    #[arg(value_name = "PATH")]
    pub paths: Vec<PathBuf>,

    /// Lint the markdown staged for commit. For a pre-commit hook.
    ///
    /// The hook `drep init` writes uses this. Running the command bare would
    /// also work - it is ~10 ms over a repository this size - but it reports
    /// findings in documents the commit never touched, and per-commit noise
    /// about someone else's file is how a report-only gate gets switched off.
    #[arg(long)]
    pub staged: bool,

    /// Exit non-zero when any check fires. Shorthand for `--fail-on info`.
    #[arg(long, conflicts_with = "fail_on")]
    pub strict: bool,

    /// Exit non-zero when a check at or above this severity fires.
    ///
    /// Severity here answers one question: does the finding change how the
    /// document renders? An unclosed fence does - everything below it becomes
    /// code - so it alone is `error`. A malformed heading or link renders
    /// wrong, so those are `warning`. Whitespace and line length render
    /// identically, so they are `info`. `--fail-on error` is therefore the
    /// calibration a hook wants: it blocks on the defect that breaks the
    /// document and stays quiet about trailing spaces.
    #[arg(long, value_name = "SEVERITY", value_parser = severity_parser())]
    pub fail_on: Option<Severity>,
}

impl LintDocsArgs {
    /// The severity at or above which a finding fails the run, if any.
    ///
    /// One place resolves `--strict` to a threshold, so the gate never has to
    /// know that two flags exist. `--strict` is `--fail-on info` because
    /// `Info` is the bottom of the vocabulary: every finding is at or above
    /// it, which is precisely what "block on anything" means.
    pub fn threshold(&self) -> Option<Severity> {
        self.fail_on.or(self.strict.then_some(Severity::Info))
    }
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
    /// What the threshold did to these findings.
    ///
    /// Here for the same reason `exit` is: the footer has to say whether the
    /// findings on screen blocked the run, and computing that a second time in
    /// the renderer is the identical mistake one field up. It is not
    /// derivable from `exit` either - a run with an unreadable file exits
    /// `Unanalyzed` whether or not its findings also crossed the threshold.
    pub gating: Gating,
}

/// What a run's threshold did to its findings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gating {
    /// No threshold: findings are reported and nothing blocks.
    ReportOnly,
    /// A threshold was in force and no finding reached it.
    NoneReached(Severity),
    /// A threshold was in force and at least one finding reached it.
    Blocked,
}

/// One `lint-docs` invocation.
///
/// `async` for exactly one reason, and it is not concurrency: `--staged` asks
/// git which documents this commit touches. Every other mode reads files and
/// runs pure checks with nothing to await.
pub async fn run(args: &LintDocsArgs, root: &Path) -> Result<Exit> {
    let outcome = outcome_for(args, root).await?;
    render::render(&outcome)?;
    Ok(outcome.exit)
}

/// The outcome for one invocation, in whichever input mode it names.
///
/// `async` for exactly one reason: `--staged` asks git which documents this
/// commit touches, through [`crate::diff`], which is the single place in the
/// binary that invokes git. Everything else here is synchronous file reading
/// and pure checks.
pub(crate) async fn outcome_for(args: &LintDocsArgs, root: &Path) -> Result<LintOutcome> {
    if !args.staged {
        return Ok(analyze(args, root));
    }

    // Straight to the reader, not through `files::expand_named`: git has
    // already answered both questions the expander exists to answer - these
    // paths exist and they are markdown - and the expander resolves an *empty*
    // list to `root`. That default is what makes bare `drep lint-docs` mean
    // "this tree", and reusing it here would turn "this commit touches no
    // markdown" into "lint every document in the repository", on every commit.
    let staged = crate::diff::staged_files(root, files::is_markdown).await?;
    Ok(analyze_files(
        staged.into_iter().map(|p| root.join(p)).collect(),
        BTreeMap::new(),
        args.threshold(),
    ))
}

/// Resolve the paths, read each file, run the checks, and gate.
///
/// Split from [`run`] so a test asserts on the outcome without capturing
/// stdout, and so the rendering has nothing to decide.
fn analyze(args: &LintDocsArgs, root: &Path) -> LintOutcome {
    let mut failures: BTreeMap<PathBuf, FailureReason> = BTreeMap::new();
    let targets = resolve(&args.paths, root, &mut failures);
    analyze_files(targets, failures, args.threshold())
}

/// Read each target, run the checks, and gate.
///
/// Takes the failures already collected rather than starting empty: the path
/// expansion rejects some of what the user named, and those rejections outrank
/// every finding this function can produce.
fn analyze_files(
    targets: Vec<PathBuf>,
    mut failures: BTreeMap<PathBuf, FailureReason>,
    threshold: Option<Severity>,
) -> LintOutcome {
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

    let gating = gating(&findings, threshold);
    let exit = gate(&failures, gating);
    LintOutcome {
        findings,
        failures,
        exit,
        gating,
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

/// What the threshold did to these findings.
///
/// The comparison is `findings::any_at_or_above`, shared with `check`: two
/// commands in one binary that disagree about which findings a severity
/// threshold covers are two contracts a hook author has to learn.
fn gating(findings: &[Finding], threshold: Option<Severity>) -> Gating {
    match threshold {
        None => Gating::ReportOnly,
        Some(threshold) if findings::any_at_or_above(findings, threshold) => Gating::Blocked,
        Some(threshold) => Gating::NoneReached(threshold),
    }
}

/// Failures outrank findings.
///
/// The precedence matches `check`'s, deliberately: a run that did not read a
/// file the user named has to say so even when it also found issues, or they
/// fix the issues and never learn about the file.
fn gate(failures: &BTreeMap<PathBuf, FailureReason>, gating: Gating) -> Exit {
    if !failures.is_empty() {
        return Exit::Unanalyzed;
    }
    match gating {
        Gating::Blocked => Exit::FoundIssues,
        Gating::ReportOnly | Gating::NoneReached(_) => Exit::Clean,
    }
}

#[cfg(test)]
mod tests;
