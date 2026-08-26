//! The `drep check` command line.
//!
//! Its own file because it is the published surface rather than any of the
//! orchestration: the flags, their groups and the prose a user reads in `--help`.
//! The orchestrator in the parent reads them and never redefines what one means.

use std::path::PathBuf;

use clap::{ArgGroup, Args};

use crate::analysis::findings::Severity;
use crate::cli::{OutputFormat, severity_parser};

#[derive(Debug, Args)]
// One rule, stated once. Paired `conflicts_with_all` attributes say the same
// thing from each side and have to be kept in agreement; a fourth input mode
// would mean editing every existing one, and missing a single edit silently
// permits an illegal combination.
// Deliberately NOT `.required(true)`. Bare `drep check` is a supported
// invocation meaning "the whole tree": `input::resolve` expands `root` through
// `files::expand_paths`, exactly as an explicit `.` would. Requiring one of the
// three would turn the plainest invocation into a usage error. Pinned by
// `bare_check_with_no_paths_expands_the_root_instead_of_reading_a_directory`,
// which exists because an earlier version passed the root through as a *file*
// and exited 2 without analyzing anything.
#[command(
    group(
        ArgGroup::new("input")
            .args(["paths", "staged", "diff", "pre_commit_push"])
            .multiple(false)
    ),
    group(ArgGroup::new("cache_mode").args(["cache_only", "push_gate"]).multiple(false))
)]
pub struct CheckArgs {
    /// Files or directories to check. Duplicates and overlaps are collapsed,
    /// so `drep check a.rs .` analyzes `a.rs` once.
    #[arg(value_name = "PATH")]
    pub paths: Vec<PathBuf>,

    /// Check the files staged for commit. For a pre-commit hook.
    #[arg(long)]
    pub staged: bool,

    /// Check the files changed since REF, e.g. `origin/main`. For pre-push.
    #[arg(long, value_name = "REF")]
    pub diff: Option<String>,

    /// The commit to diff *to*. Defaults to `HEAD`. Only valid with `--diff`.
    ///
    /// A pre-push hook needs this: git can push a ref that is not the
    /// checked-out one (`git push origin feature:feature` from another branch,
    /// or `git push --all`), and diffing to `HEAD` there reviews a different
    /// branch and lets the pushed code through unseen.
    #[arg(long, value_name = "REF", requires = "diff")]
    pub tip: Option<String>,

    /// Read the pushed base and tip from pre-commit's hook environment.
    ///
    /// Used by the published `drep-check-push` hook. pre-commit otherwise
    /// passes filenames, which would make drep review whole files instead of
    /// the hunks between `PRE_COMMIT_FROM_REF` and `PRE_COMMIT_TO_REF`.
    #[arg(long)]
    pub pre_commit_push: bool,

    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub format: OutputFormat,

    /// Also block on LLM findings at or above this severity.
    ///
    /// Deterministic tool findings always block; this opts the LLM's findings
    /// into gating too. Left unset, they inform without blocking - which is
    /// the useful default, because the model emits style suggestions on
    /// nearly every file.
    #[arg(long, value_name = "SEVERITY", value_parser = severity_parser())]
    pub fail_on: Option<Severity>,

    /// Use cached LLM reviews only; never contact a provider.
    ///
    /// An uncached file exits 3 without warming it; run a normal check to
    /// populate the missing entry. The generated pre-push hook uses
    /// `--push-gate` for the full warm-and-reconnect handshake.
    #[arg(long)]
    pub cache_only: bool,

    /// Prepare a push without resuming a stale remote connection.
    ///
    /// Cached reviews pass immediately. A cold review is completed and cached,
    /// then exits 3 so Git reconnects; repeating `git push` uses the cache.
    #[arg(long)]
    pub push_gate: bool,

    /// Override the repository's maximum fresh LLM review rounds.
    #[arg(
        long,
        value_name = "N",
        value_parser = clap::value_parser!(u32).range(1..),
        conflicts_with = "unlimited_reviews"
    )]
    pub max_review_rounds: Option<u32>,

    /// Permit fresh LLM review rounds without a limit for this invocation.
    #[arg(long)]
    pub unlimited_reviews: bool,
}
