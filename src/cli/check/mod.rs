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
// `pub(crate)` for `READ_MAX_BYTES` alone: the guard's relationship to
// `analysis::payload::PAYLOAD_MAX_BYTES` is load-bearing (it must never sit
// below it), and the assertion pinning that has to see both constants.
pub(crate) mod input;
mod render;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use clap::{ArgGroup, Args};

use crate::analysis::findings::{self, Finding, Severity};
use crate::analysis::result::{FailureReason, union_failures};
use crate::auth;
use crate::cli::{OutputFormat, severity_parser};
use crate::config;
use crate::llm::cache::Cache;
use crate::llm::chain::ProviderChain;

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
    group(ArgGroup::new("input").args(["paths", "staged", "diff"]).multiple(false)),
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
    /// Which providers answered, and for how many files.
    ///
    /// Empty when the LLM layer produced nothing at all. Only providers that
    /// served at least one file appear, in chain order, so a run that never
    /// left the head is one entry - and a run that did fall through says so
    /// rather than leaving a silent switch from a local model to a paid one.
    pub provider_uses: Vec<ProviderUse>,
    /// A cold push review completed successfully and must be retried over a new
    /// Git transport. False for ordinary checks and warm push gates.
    pub retry_push: bool,
    /// The gate's verdict for this run.
    ///
    /// On the outcome rather than passed alongside it: `render` used to take
    /// an `Exit` as a separate argument, which let the two disagree - and they
    /// did, because `render` computed its own and ignored `--fail-on`. A field
    /// set once by `run` makes the mismatch unrepresentable.
    pub exit: Exit,
}

/// One provider's share of a run.
///
/// The backend location is carried, not just the model, because "gpt-5.6-sol" does not
/// tell a user whether they paid for it - two entries can name the same model
/// at a local proxy and at the vendor.
pub struct ProviderUse {
    /// Zero-based position in the chain. Rendered one-based.
    pub index: usize,
    pub model: String,
    pub location: String,
    pub files: usize,
}

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
    run_against(args, root, cache, &auth::default_path()?).await
}

/// `run_with`, against a named auth store.
///
/// The store is user-level state outside the repository, so it is a parameter
/// for the same reason `root` is one: a test using the real one reads whatever
/// the developer has stored, and a config that omits `api_key` would then
/// behave differently on their machine than in CI. `init::run_with` and
/// `auth::run_at` already thread it for that reason; this was the one command
/// that did not.
pub(crate) async fn run_against(
    args: &CheckArgs,
    root: &Path,
    cache: Cache,
    auth_path: &Path,
) -> Result<Exit> {
    // Anchored on `root`, not the process cwd. `config::default_config_path()`
    // resolves against the cwd, which would make `root` a half-truth: input
    // resolution would read one directory and configuration another. The CLI
    // passes ".", so production behaviour is unchanged, and a test can point
    // the whole run at a `TempDir` without chdir-ing a shared process.
    let default_config_path = config::default_config_path();
    if default_config_path.is_absolute() {
        return Err(anyhow!(
            "default config path must be repository-relative, got {}",
            default_config_path.display()
        ));
    }
    let config_path = root.join(default_config_path);
    let mut config = config::load(&config_path)
        .with_context(|| format!("could not load {}", config_path.display()))?;

    // Fill in the keys the file left unset from the user-level store. An
    // explicit `api_key` in `drep.toml` always wins, so this cannot change what
    // an existing config does; it only supplies what `drep init` stopped writing
    // into the repository. A store that cannot be read is fatal rather than
    // treated as empty - running the gate unauthenticated would surface as a 401
    // per file, which reads as a broken key rather than a broken store.
    let store = auth::AuthStore::load(auth_path)
        .with_context(|| format!("could not read the auth store at {}", auth_path.display()))?;
    auth::resolve(&mut config, &store);
    // The whole enabled chain, not just its head: `providers()` is the
    // failover order, and `load` has already rejected a config with none.
    // The `is_empty` guard stays because `Config` is constructible without
    // going through `load`, and a panic inside the commit gate is a worse
    // failure than a message naming the file. The error is `load`'s own rather
    // than a second sentence written here: two hand-written copies of "no
    // provider configured" drift, and the copy that lived here had already
    // lost the actionable "run `drep init`" half.
    let providers = config.providers();
    if providers.is_empty() {
        return Err(config::ConfigError::NoProviders(config_path.clone()).into());
    }

    // Resolved *after* the config, because a missing config is fatal and
    // resolution is not free: in `--staged` mode it spawns git, and in paths
    // mode it reads every target file into memory. Discovering "no drep.toml"
    // afterwards means paying for all of it and throwing it away.
    let work = input::resolve(args, root)
        .await
        .with_context(|| format!("could not resolve input under {}", root.display()))?;

    let chain =
        ProviderChain::new(&providers).map_err(|e| anyhow!("could not build LLM analyzer: {e}"))?;
    let analyzer = crate::analysis::code_quality::CodeQualityAnalyzer::new(chain, cache.clone())
        .with_cache_only(args.cache_only || args.push_gate);

    // The two layers share no data - they only both report failures - so they
    // run concurrently. The deterministic leg is pure added latency otherwise:
    // a warm `cargo clippy` on this repo is ~3.5s, and the LLM leg is
    // multi-second regardless, so joining hides essentially all of it.
    let (deterministic_result, llm_result) = tokio::join!(
        deterministic::run(&work, root),
        analyzer.analyze_files(&work.by_file),
    );
    let (tool_findings, tool_failures) = deterministic_result;
    let mut llm_result = llm_result;

    let cache_misses_only = !llm_result.failed_files.is_empty()
        && llm_result
            .failed_files
            .values()
            .all(|reason| matches!(reason, FailureReason::CacheMiss));
    let warmed_for_push = args.push_gate
        && cache_misses_only
        && work.read_failures.is_empty()
        && tool_failures.is_empty()
        && tool_findings.is_empty();
    if warmed_for_push {
        let misses: Vec<&[_]> = work
            .by_file
            .iter()
            .filter(|hunks| {
                hunks.first().is_some_and(|first| {
                    matches!(
                        llm_result.failed_files.get(&first.file_path),
                        Some(FailureReason::CacheMiss)
                    )
                })
            })
            .map(Vec::as_slice)
            .collect();
        for hunks in &misses {
            if let Some(first) = hunks.first() {
                llm_result.failed_files.remove(&first.file_path);
            }
        }
        llm_result.merge(analyzer.analyze_files_live(&misses).await);
    }
    let provider_uses = provider_uses(analyzer.chain());

    // All concurrent writers have finished, so one oldest-first pass can
    // enforce the configured disk ceiling without racing another put. Cache
    // maintenance is best-effort: an unreadable cache must never turn an
    // otherwise valid review into an unanalyzed file.
    let _ = cache.evict_if_needed();

    let mut failures: BTreeMap<PathBuf, FailureReason> = BTreeMap::new();
    // Union order is the reporting priority: a file that could not be read
    // keeps that reason over a later layer's view of the same path.
    union_failures(&mut failures, work.read_failures);
    union_failures(&mut failures, tool_failures);
    union_failures(&mut failures, llm_result.failed_files);

    let mut outcome = CheckOutcome {
        tool_findings,
        llm_findings: llm_result.findings,
        failures,
        provider_uses,
        retry_push: false,
        exit: Exit::Clean,
    };
    outcome.exit = gate(&outcome, args.fail_on);
    if warmed_for_push && outcome.exit == Exit::Clean {
        outcome.retry_push = true;
        outcome.exit = Exit::CacheMiss;
    }

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
    if outcome
        .failures
        .values()
        .any(|reason| !matches!(reason, FailureReason::CacheMiss))
    {
        return Exit::Unanalyzed;
    }
    if any_blocking_tool_finding(&outcome.tool_findings) {
        return Exit::FoundIssues;
    }
    if let Some(threshold) = fail_on
        && findings::any_at_or_above(&outcome.llm_findings, threshold)
    {
        return Exit::FoundIssues;
    }
    // The earlier arm returned for every non-cache failure, so only cache
    // misses remain here. Findings deliberately outrank a retryable miss.
    if !outcome.failures.is_empty() {
        return Exit::CacheMiss;
    }
    Exit::Clean
}

/// A tool finding always blocks. A deterministic tool's verdict is the
/// project's own choice, so the gate honors it without an allow-list.
fn any_blocking_tool_finding(findings: &[Finding]) -> bool {
    !findings.is_empty()
}

/// Who answered, and for how many files.
///
/// Read off the chain, which counted as it went - the counts never leave the
/// object that produced them, so they cannot drift from the demotion state
/// sitting beside them, and `AnalysisResult` needs no per-provider field whose
/// merge rule would be the one exception to its union-not-sum invariant.
/// Providers that served nothing are omitted: the report answers "who reviewed
/// this code", and an untouched fallback did not.
fn provider_uses(chain: &ProviderChain) -> Vec<ProviderUse> {
    chain
        .providers()
        .iter()
        .enumerate()
        .filter(|(_, provider)| provider.served() > 0)
        .map(|(index, provider)| ProviderUse {
            index,
            model: provider.model().to_owned(),
            location: provider.location().to_owned(),
            files: provider.served(),
        })
        .collect()
}

/// The crate's `Exit` re-exported so `cli::check::run` returns the same
/// type the upper layer expects, without leaking the orchestrator's
/// dependency path.
pub use crate::Exit;

#[cfg(test)]
mod tests;
