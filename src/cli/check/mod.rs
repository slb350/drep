//! `drep check` - the commit gate, end to end.
//!
//! Three concerns, three sibling modules, one orchestrator here:
//!
//! - `input` resolves the four input modes (paths, `--staged`, `--diff`, and
//!   pre-commit's pre-push ref environment)
//!   into a uniform `[Hunk]` list. Whatever the caller passed, the rest of
//!   the pipeline sees the same shape.
//! - `deterministic` runs the configured per-language tools, collecting
//!   their findings AND marking every file in a batch failed when a tool was
//!   `Unavailable` — the per-tool/per-file join the exit-2 contract rests on.
//! - `render` turns the two layers' findings plus the failure map into the
//!   text or JSON output the user sees.
//! - `refusal` answers whether site policy permits a semantic layer here at
//!   all, and owns the ordering that keeps a refused repository from ever
//!   reaching a credential, a chain or the cache.
//!
//! The split is by topic, not by file size, because the smallest meaningful
//! unit of `check` is "one input mode" or "one output format" and the
//! dependencies between those are weak. The orchestrator ([`run`]) is the
//! only place where the layers meet, which is what the loading-order
//! invariants and the exit-code precedence are pinned against.

mod args;
mod deterministic;
// `pub(crate)` for `READ_MAX_BYTES` alone: the guard's relationship to
// `analysis::payload::PAYLOAD_MAX_BYTES` is load-bearing (it must never sit
// below it), and the assertion pinning that has to see both constants.
pub(crate) mod input;
mod refusal;
mod render;
mod review_budget;
mod semantic;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};

use crate::analysis::acknowledgements;
use crate::analysis::findings::{self, Finding, Severity};
use crate::analysis::result::{FailureReason, union_failures};
use crate::auth;
use crate::cli::MachineFiles;
use crate::config;
use crate::llm::cache::Cache;
use crate::llm::chain::ProviderChain;
use review_budget::Budget;

pub use args::CheckArgs;

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
    /// What this invocation did to the bounded semantic-review cycle.
    pub review_activity: Option<ReviewActivity>,
    /// The gate's verdict for this run.
    ///
    /// On the outcome rather than passed alongside it: `render` used to take
    /// an `Exit` as a separate argument, which let the two disagree - and they
    /// did, because `render` computed its own and ignored `--fail-on`. A field
    /// set once by `run` makes the mismatch unrepresentable.
    pub exit: Exit,
}

/// Visible accounting for a fresh semantic review.
pub enum ReviewActivity {
    /// Actionable findings survived suppression and acknowledgement, consuming
    /// one remediation round.
    Counted { round: u32, limit: u32 },
    /// An authoritative clean result completed the current review cycle.
    Reset,
    /// The caller explicitly disabled the configured bound for this review.
    Unlimited,
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
    run_against(
        args,
        root,
        cache,
        &MachineFiles {
            auth: &auth::default_path()?,
            policy: &config::site::default_path(),
        },
    )
    .await
}

/// `run_with`, against a named auth store and a named site policy file.
///
/// Both are machine-level state outside the repository, so both are parameters
/// for the same reason `root` is one: a test using the real ones reads whatever
/// the developer's machine happens to hold, and a repository would then behave
/// differently there than in CI. `init::run_with` and `auth::run_at` already
/// thread the store for that reason; the policy file follows the same seam
/// rather than being read inside the call.
///
/// They arrive as one [`MachineFiles`] rather than two `&Path`
/// positionals because the two are the same type and a transposition compiles;
/// see that struct for what the swap silently disables.
pub(crate) async fn run_against(
    args: &CheckArgs,
    root: &Path,
    cache: Cache,
    machine: &MachineFiles<'_>,
) -> Result<Exit> {
    // Read before the repository's own config, and before a byte of source. A
    // machine whose policy file is broken must not then run whatever the
    // repository says, so a policy failure outranks a repo-config failure. No
    // `.with_context`: unlike `ConfigError::Io`, the message already names the
    // file and states the consequence, and a context line would say it twice.
    let site = config::site::load(machine.policy)?;

    let (config_path, mut config) = configured(root, site.as_ref())?;

    // Resolved *after* the config, because a missing config is fatal and
    // resolution is not free: in `--staged` mode it spawns git, and in paths
    // mode it reads every target file into memory. Discovering "no drep.toml"
    // afterwards means paying for all of it and throwing it away.
    //
    // And *before* the refusal, because the refusal is a question about the
    // files this run would review rather than about `root` alone - see
    // `refusal::reviewed_directories`. Resolution reads local files and contacts
    // nothing, so the ordering the refusal owns is untouched by it.
    let work = input::resolve(args, root)
        .await
        .with_context(|| format!("could not resolve input under {}", root.display()))?;

    // Fill in the keys the file left unset, and build the analyzer - unless site
    // policy refuses review of this repository, in which case none of that
    // happens at all. `refusal` owns that ordering; see its module doc.
    let authoritative = review_budget::is_authoritative(args);
    let source = refusal::source(
        &refusal::Locations {
            root,
            config: &config_path,
            machine,
        },
        &mut config,
        site.as_ref(),
        &work,
        cache.clone(),
        args.cache_only || args.push_gate || authoritative,
    )
    .await?;

    let acknowledgements = acknowledgements::Store::load(root)?;

    let effective_limit = args.max_review_rounds.unwrap_or(config.max_review_rounds);
    let semantic_policy = semantic::Policy {
        authoritative,
        limit: effective_limit,
    };

    // The two layers share no data - they only both report failures - so they
    // run concurrently. The deterministic leg is pure added latency otherwise:
    // a warm `cargo clippy` on this repo is ~3.5s, and the LLM leg is
    // multi-second regardless, so joining hides essentially all of it.
    //
    // A refusal has no second leg to overlap with. The deterministic tools still
    // run and still gate: they are local, they contact nothing, and they are the
    // half of drep that works without a model.
    let (deterministic_result, semantic_pass, eligible_push_warm) = match &source {
        refusal::Source::Refused(refusal) => (
            deterministic::run(&work, root).await,
            semantic::refused(&work, refusal),
            false,
        ),
        refusal::Source::Analyze(analyzer) => {
            analyzed(args, root, &work, analyzer, semantic_policy).await?
        }
    };
    let (tool_findings, tool_failures, compiled_files) = deterministic_result;
    let semantic::Pass {
        cached: mut llm_result,
        live: mut live_result,
        live_review,
        budget,
        should_review_live,
        limit_reached,
        live_answered,
    } = semantic_pass;
    // Empty for a refusal, and deliberately: the report answers "who reviewed
    // this code", and nobody did.
    let provider_uses = match &source {
        refusal::Source::Refused(_) => Vec::new(),
        refusal::Source::Analyze(analyzer) => provider_uses(analyzer.chain()),
    };

    // All concurrent writers have finished, so one oldest-first pass can
    // enforce the configured disk ceiling without racing another put. Cache
    // maintenance is best-effort: an unreadable cache must never turn an
    // otherwise valid review into an unanalyzed file. Unconditional under a
    // refusal too: it bounds a user-level directory and reads no verdict for
    // this repository, which is the only thing the refusal is about.
    let _ = cache.evict_if_needed();

    adjudicate_findings(
        &mut llm_result.findings,
        &compiled_files,
        &work,
        &acknowledgements,
    );
    adjudicate_findings(
        &mut live_result.findings,
        &compiled_files,
        &work,
        &acknowledgements,
    );

    let mut review_activity = None;
    match live_review {
        semantic::LiveReview::Reserved(claim) => {
            if !live_result.findings.is_empty() {
                let round = claim.round();
                claim.commit()?;
                review_activity = Some(ReviewActivity::Counted {
                    round,
                    limit: effective_limit,
                });
            }
            // Otherwise Drop refunds the pending slot. Pure failures did not
            // complete a review, and a clean answer consumes no remediation round.
        }
        semantic::LiveReview::Unbounded
            if should_report_unlimited(args.unlimited_reviews, live_answered) =>
        {
            review_activity = Some(ReviewActivity::Unlimited);
        }
        semantic::LiveReview::Skip
        | semantic::LiveReview::Unbounded
        | semantic::LiveReview::Denied { .. } => {}
    }
    llm_result.merge(live_result);

    if review_budget::is_completion_scope(args)
        && !args.cache_only
        && !work.by_file.is_empty()
        && work.read_failures.is_empty()
        && tool_findings.is_empty()
        && tool_failures.is_empty()
        && llm_result.findings.is_empty()
        && llm_result.failed_files.is_empty()
    {
        // Reset is an authoritative quota-state transition, not cache
        // maintenance. Failing it closed keeps this result from claiming a
        // cycle reset that the next invocation cannot observe.
        let reset = if let Some(budget) = &budget {
            budget.reset()?
        } else {
            Budget::for_repo(root, effective_limit).await?.reset()?
        };
        if should_report_reset(live_answered, reset) {
            review_activity = Some(ReviewActivity::Reset);
        }
    }

    let warmed_for_push = eligible_push_warm && should_review_live && !limit_reached;

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
        review_activity,
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

/// What the deterministic leg produced: findings, failures, and the files a
/// configured compiler successfully checked.
type Deterministic = (
    Vec<Finding>,
    BTreeMap<PathBuf, FailureReason>,
    BTreeSet<PathBuf>,
);

/// `drep.toml` as this run will use it: loaded from under `root`, then lowered to
/// what the site allows.
///
/// One named function rather than three steps inside `run_against`, so a test can
/// observe the clamp reaching a config a real run would use. Every ceiling test
/// called [`config::site::SiteConfig::apply`] directly, so deleting the call from
/// the orchestrator left the whole suite green while `max_concurrent_ceiling`
/// constrained nothing - and `doctor`, which computes its note from the raw TOML
/// tree, went on reporting the clamp as enforced.
///
/// The path is returned beside the config because the caller needs it for the
/// no-providers error, which names the file the user has to edit.
fn configured(
    root: &Path,
    site: Option<&config::site::SiteConfig>,
) -> Result<(PathBuf, config::Config)> {
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

    // Applied before anything reads a provider: a checkout may lower its own
    // concurrency but not raise it past what the site allows. Nothing is printed
    // here - a clamp is not an error, and `doctor` is where it is reported.
    if let Some(site) = site {
        site.apply(&mut config);
    }
    Ok((config_path, config))
}

/// Both legs, for a repository site policy permits review of.
///
/// Split out of `run_against` so the refusal is a two-arm match there rather than
/// a flag threaded through this. It also keeps `semantic::Stage::Deferred` unable
/// to escape the one scope that holds an analyzer: the push gate defers its live
/// pass until the deterministic verdict is in, and there is no analyzer to resume
/// it with in the refused arm.
async fn analyzed(
    args: &CheckArgs,
    root: &Path,
    work: &input::Work,
    analyzer: &crate::analysis::code_quality::CodeQualityAnalyzer,
    policy: semantic::Policy,
) -> Result<(Deterministic, semantic::Pass, bool)> {
    let semantic = async {
        let cached = analyzer.analyze_files(&work.by_file).await;
        if args.push_gate {
            Ok(semantic::Stage::Deferred(cached))
        } else {
            semantic::complete(args, root, work, analyzer, policy, cached, true)
                .await
                .map(Box::new)
                .map(semantic::Stage::Complete)
        }
    };
    let (deterministic, stage) = tokio::join!(deterministic::run(work, root), semantic);
    let (tool_findings, tool_failures, compiled_files) = deterministic;
    let (pass, eligible) = match stage? {
        semantic::Stage::Deferred(cached) => {
            let eligible = push_warm_eligible(
                &cached,
                work.read_failures.is_empty(),
                tool_failures.is_empty(),
                tool_findings.is_empty(),
            );
            (
                semantic::complete(args, root, work, analyzer, policy, cached, eligible).await?,
                eligible,
            )
        }
        semantic::Stage::Complete(pass) => (*pass, false),
    };
    Ok((
        (tool_findings, tool_failures, compiled_files),
        pass,
        eligible,
    ))
}

fn push_warm_eligible(
    cached: &crate::analysis::result::AnalysisResult,
    reads_clean: bool,
    tools_analyzed: bool,
    tools_clean: bool,
) -> bool {
    cached.has_failures()
        && cached
            .failed_files
            .values()
            .all(|reason| matches!(reason, FailureReason::CacheMiss))
        && reads_clean
        && tools_analyzed
        && tools_clean
}

fn should_report_unlimited(requested: bool, live_answered: bool) -> bool {
    requested && live_answered
}

fn should_report_reset(live_answered: bool, state_removed: bool) -> bool {
    live_answered || state_removed
}

/// Apply the two repository-grounded filters in their load-bearing order.
fn adjudicate_findings(
    findings: &mut Vec<Finding>,
    compiled: &BTreeSet<PathBuf>,
    work: &input::Work,
    acknowledgements: &acknowledgements::Store,
) {
    suppress_disproved_compile_claims(findings, compiled);
    acknowledgements::apply(findings, &work.by_file, acknowledgements);
}

/// Drop only findings that explicitly claim compilation failure after a
/// configured compiler has successfully checked the same file.
fn suppress_disproved_compile_claims(findings: &mut Vec<Finding>, compiled: &BTreeSet<PathBuf>) {
    findings.retain(|finding| {
        !(finding.asserts_compile_failure && compiled.contains(Path::new(&finding.file_path)))
    });
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
