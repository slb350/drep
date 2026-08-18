//! `drep check` - the commit gate, end to end.
//!
//! Three concerns, three sibling modules, one orchestrator here:
//!
//! - [`input`] resolves the three input modes (paths, `--staged`, `--diff`)
//!   into a uniform `[Hunk]` list. Whatever the caller passed, the rest of
//!   the pipeline sees the same shape.
//! - [`deterministic`] runs the configured per-language tools, collecting
//!   their findings AND marking every file in a batch failed when a tool was
//!   `Unavailable` — the per-tool/per-file join the exit-2 contract rests on.
//! - [`render`] turns the two layers' findings plus the failure map into the
//!   text or JSON output the user sees.
//!
//! The split is by topic, not by file size, because the smallest meaningful
//! unit of `check` is "one input mode" or "one output format" and the
//! dependencies between those are weak. The orchestrator ([`run`]) is the
//! only place where the layers meet, which is what the loading-order
//! invariants and the exit-code precedence are pinned against.

mod deterministic;
mod input;
mod render;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use clap::builder::{PossibleValuesParser, TypedValueParser};
use clap::{ArgGroup, Args};

use crate::analysis::findings::{Finding, Severity};
use crate::analysis::result::{FailureReason, union_failures};
use crate::cli::OutputFormat;
use crate::config::{self, LlmConfig};
use crate::llm::cache::Cache;
use crate::llm::concurrency::Limiter;

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
#[command(group(ArgGroup::new("input").args(["paths", "staged", "diff"]).multiple(false)))]
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
}

/// Parse `--fail-on` from the severity vocabulary.
///
/// Built from `Severity::ALL` rather than a literal list, so `--help` shows
/// exactly the values `FromStr` accepts and neither can drift.
fn severity_parser() -> impl TypedValueParser<Value = Severity> {
    PossibleValuesParser::new(Severity::ALL.map(Severity::as_str))
        .map(|name| name.parse::<Severity>().expect("possible values parse"))
}

/// Everything one `check` run produced.
///
/// The two layers stay in separate fields all the way to rendering. Their
/// findings are gated differently - deterministic ones always block, LLM ones
/// only under `--fail-on` - and keeping them apart makes that structural
/// rather than a tag on `Finding` that a caller could read wrong.
pub struct CheckOutcome {
    /// Findings from the configured deterministic tools. These always block.
    pub tool_findings: Vec<Finding>,
    /// Findings from the LLM. These only block under `--fail-on`.
    pub llm_findings: Vec<Finding>,
    /// Files that went unanalyzed for any reason. The two layers cover the
    /// same files, so the CLI unions them; on a key collision the first
    /// reason wins, matching `AnalysisResult::merge`.
    pub failures: BTreeMap<PathBuf, FailureReason>,
    /// The gate's verdict for this run.
    ///
    /// On the outcome rather than passed alongside it: `render` used to take
    /// an `Exit` as a separate argument, which let the two disagree - and they
    /// did, because `render` computed its own and ignored `--fail-on`. A field
    /// set once by `run` makes the mismatch unrepresentable.
    pub exit: Exit,
}

/// The ceiling at which a file read off disk is still accepted whole.
///
/// 256 KiB matches the payload layer's "one chunk" feel and is small enough
/// that a synthetic whole-file hunk never bloats the LLM request on a
/// realistic project. Files above this become `FailureReason::TooLarge`
/// rather than a silent skip - a file drep declined to analyze is not clean.
pub const WHOLE_FILE_MAX_BYTES: u64 = 256 * 1024;

/// One `check` invocation: resolve input, run both layers, gate, render, return
/// `Exit`.
///
/// `root` is the working directory the input resolution runs against. The CLI
/// passes `Path::new(".")`, the tests pass a `TempDir` so each one stands
/// alone. Failure to load the config is a hard error: the LLM is mandatory in
/// 2.x and there is no deterministic-only mode.
pub async fn run(args: &CheckArgs, root: &Path) -> Result<Exit> {
    run_with(
        args,
        root,
        Cache::new(Cache::default_root(), CACHE_TTL_DAYS, CACHE_MAX_BYTES),
    )
    .await
}

/// How long a cached LLM response stays valid, and how large the store may
/// grow. Documented tunables rather than magic numbers at the call site.
pub const CACHE_TTL_DAYS: u64 = 30;
/// See [`CACHE_TTL_DAYS`].
pub const CACHE_MAX_BYTES: u64 = 256 * 1024 * 1024;

/// `run`, with the response cache supplied.
///
/// Split out so a test can point the cache at its own `TempDir`. The cache
/// was built inside the run from `Cache::default_root()`, which is the user's
/// real cache directory - so every test run wrote to it, and a stale entry
/// from one test satisfied another: an unreachable-endpoint test once exited 0
/// because a previous test had cached a clean response for the same payload.
pub(crate) async fn run_with(args: &CheckArgs, root: &Path, cache: Cache) -> Result<Exit> {
    // Anchored on `root`, not the process cwd. `config::default_config_path()`
    // resolves against the cwd, which would make `root` a half-truth: input
    // resolution would read one directory and configuration another. The CLI
    // passes ".", so production behaviour is unchanged, and a test can point
    // the whole run at a `TempDir` without chdir-ing a shared process.
    let config_path = root.join(config::default_config_path());
    let config = config::load(&config_path)
        .with_context(|| format!("could not load {}", config_path.display()))?;

    // Resolved *after* the config, because a missing config is fatal and
    // resolution is not free: in `--staged` mode it spawns git, and in paths
    // mode it reads every target file into memory. Discovering "no drep.toml"
    // afterwards means paying for all of it and throwing it away.
    let work = input::resolve(args, root)
        .await
        .with_context(|| format!("could not resolve input under {}", root.display()))?;

    let mut failures: BTreeMap<PathBuf, FailureReason> = BTreeMap::new();
    // Disk and size failures from the input layer are the first writer into
    // the failure map. The orchestrator's first-wins rule means a file that
    // could not be read stays "unreadable" even if the LLM layer later
    // reports the same path with a different reason — read failures are
    // load-bearing, and a later layer's view is not allowed to overwrite
    // them.
    union_failures(&mut failures, work.read_failures.clone());

    // The two layers share no data - they only both report failures - so they
    // run concurrently. The deterministic leg is pure added latency otherwise:
    // a warm `cargo clippy` on this repo is ~3.5s, and the LLM leg is
    // multi-second regardless, so joining hides essentially all of it.
    let (deterministic_result, llm_result) = tokio::join!(
        deterministic::run(&work, root),
        run_llm(&work, &config.llm, cache),
    );
    let (tool_findings, tool_failures) = deterministic_result;
    let (llm_findings, llm_failures) = llm_result?;

    // Union order is the reporting priority: a file that could not be read
    // keeps that reason over a later layer's view of the same path.
    union_failures(&mut failures, tool_failures);
    union_failures(&mut failures, llm_failures);

    let mut outcome = CheckOutcome {
        tool_findings,
        llm_findings,
        failures,
        exit: Exit::Clean,
    };
    outcome.exit = gate(&outcome, args.fail_on);

    render::render(&outcome, args.format)?;
    Ok(outcome.exit)
}

/// What the process should exit with.
///
/// Two precedence rules, in order:
/// 1. Any failure → `Unanalyzed` (exit 2). A failure outranks a finding,
///    because the file went unanalyzed whether or not the LLM also produced
///    findings on a partial result.
/// 2. Any blocking finding → `FoundIssues` (exit 1). Tool findings always
///    block; LLM findings block when `fail_on` admits their severity and
///    `fail_on` is set.
fn gate(outcome: &CheckOutcome, fail_on: Option<Severity>) -> Exit {
    if !outcome.failures.is_empty() {
        return Exit::Unanalyzed;
    }
    if any_blocking_tool_finding(&outcome.tool_findings) {
        return Exit::FoundIssues;
    }
    if let Some(threshold) = fail_on {
        if outcome.llm_findings.iter().any(|f| f.severity >= threshold) {
            return Exit::FoundIssues;
        }
    }
    Exit::Clean
}

/// A tool finding always blocks. A deterministic tool's verdict is the
/// project's own choice, so the gate honors it without an allow-list.
fn any_blocking_tool_finding(findings: &[Finding]) -> bool {
    !findings.is_empty()
}

/// Run the LLM layer and union its failures into `failures`.
///
/// Returns the LLM findings. The analyzer is built from the loaded config,
/// the standard cache, and a limiter sized by `max_concurrent`. A failure to
/// build the analyzer (no LLM configured) is propagated as `Err` - the gate
/// is LLM-only and there is no deterministic-only fallback.
async fn run_llm(
    work: &input::Work,
    cfg: &LlmConfig,
    cache: Cache,
) -> Result<(Vec<Finding>, BTreeMap<PathBuf, FailureReason>)> {
    let limiter = Limiter::new(cfg.max_concurrent);
    let analyzer = crate::analysis::code_quality::CodeQualityAnalyzer::new(cfg, cache, limiter)
        .map_err(|e| anyhow!("could not build LLM analyzer: {e}"))?;

    let result = analyzer.analyze_files(&work.by_file).await;
    Ok((result.findings, result.failed_files))
}

/// The crate's `Exit` re-exported so `cli::check::run` returns the same
/// type the upper layer expects, without leaking the orchestrator's
/// dependency path.
pub use crate::Exit;

#[cfg(test)]
mod tests;
