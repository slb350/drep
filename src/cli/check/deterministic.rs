//! The deterministic layer of `drep check`.
//!
//! Every configured tool runs once per language, over the union of files of
//! that language. The two join operations that have to live here:
//!
//! - **Tool → files**: a `ToolStatus::Unavailable` outcome is per-tool, but
//!   `CheckOutcome::failures` is per-file. The mapping is built once,
//!   keyed by `(language, tool)`, so every file in the batch gets the same
//!   `ToolUnavailable` reason. The orchestrator unions this into the LLM
//!   layer's failures.
//! - **Skipped vs. Unavailable**: a tool that is not configured for the
//!   project is `Skipped` and contributes nothing — it never appears in
//!   `failures`. A tool that is configured but cannot run is `Unavailable`
//!   and contributes one failure per file.
//!
//! Tool invocation is batched, not per-file: a project with twenty Python
//! files would otherwise pay twenty `ruff` process starts. The batch lives
//! in `run_tool`; this module just decides which files to pass.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use futures::stream::{self, StreamExt};

use crate::analysis::findings::Finding;
use crate::analysis::result::{FailureReason, union_failures};
use crate::cli::check::input::Work;
use crate::languages;
use crate::languages::runner::{self};
use crate::languages::spec::ToolSpec;

const TOOL_PROCESS_CONCURRENCY: usize = 4;

/// Run every configured deterministic tool against the work set and return
/// its findings.
///
/// `failures` is filled in place: every file in a batch whose tool ran with
/// `ToolStatus::Unavailable` lands there with a `FailureReason::ToolUnavailable`.
/// The orchestrator unions this with the LLM layer's failures; the first
/// reason wins on a key collision, matching `AnalysisResult::merge`.
pub async fn run(
    work: &Work,
    root: &Path,
) -> (
    Vec<Finding>,
    BTreeMap<PathBuf, FailureReason>,
    BTreeSet<PathBuf>,
) {
    let mut failures: BTreeMap<PathBuf, FailureReason> = BTreeMap::new();
    let tasks = plan_tasks(work, root);
    let (serial, parallel): (Vec<_>, Vec<_>) = tasks
        .into_iter()
        .partition(|task| task.spec.serial_in_repository);
    let parallel = stream::iter(parallel)
        .map(|task| run_one(task, root))
        .buffer_unordered(TOOL_PROCESS_CONCURRENCY)
        .collect::<Vec<_>>();
    let serial = async {
        let mut outcomes = Vec::with_capacity(serial.len());
        for task in serial {
            outcomes.push(run_one(task, root).await);
        }
        outcomes
    };
    let (mut outcomes, serial_outcomes) = tokio::join!(parallel, serial);
    outcomes.extend(serial_outcomes);
    let mut findings = Vec::new();
    let mut compiled = BTreeSet::new();
    for (outcome, files) in outcomes {
        merge_outcome(outcome, files, &mut failures, &mut findings, &mut compiled);
    }
    (findings, failures, compiled)
}

/// One deterministic-tool invocation: the spec and the files it should be
/// invoked with. The tool name is on the spec, so it is not duplicated
/// here.
struct PlannedTask {
    spec: &'static ToolSpec,
    workspace_root: PathBuf,
    files: Vec<PlannedFile>,
}

struct PlannedFile {
    original: PathBuf,
    absolute: PathBuf,
    argument: String,
}

/// Run one planned task and return its outcome alongside the files it was
/// given. The files list is moved back out so the caller can map the
/// outcome back to per-file failures without re-borrowing.
async fn run_one(task: PlannedTask, root: &Path) -> (runner::ToolOutcome, Vec<PathBuf>) {
    let PlannedTask {
        spec,
        workspace_root,
        files,
    } = task;
    let mut arguments = Vec::with_capacity(files.len());
    let mut originals = Vec::with_capacity(files.len());
    let mut original_by_absolute = BTreeMap::new();
    for file in files {
        arguments.push(file.argument);
        originals.push(file.original.clone());
        original_by_absolute.insert(file.absolute, file.original);
    }
    let mut outcome = runner::run_tool_at(spec, root, &workspace_root, &arguments).await;
    // The canonical index is the second spelling `retain_requested` also
    // compares: a tool deriving paths from its own resolved cwd answers
    // through symlinks drep left alone. It is built on the first finding that
    // misses in exact form, because under an ordinary checkout none ever does
    // and resolving every planned file up front spends a `realpath` each to
    // answer a question nothing asks.
    let mut original_by_canonical: Option<BTreeMap<PathBuf, PathBuf>> = None;
    for finding in &mut outcome.findings {
        let joined = runner::joined_reported(&workspace_root, &finding.file_path);
        let original = match original_by_absolute.get(&joined) {
            Some(original) => Some(original),
            None => joined.canonicalize().ok().and_then(|resolved| {
                original_by_canonical
                    .get_or_insert_with(|| {
                        original_by_absolute
                            .iter()
                            .filter_map(|(absolute, original)| {
                                Some((absolute.canonicalize().ok()?, original.clone()))
                            })
                            .collect()
                    })
                    .get(&resolved)
            }),
        };
        if let Some(original) = original {
            finding.file_path = original.to_string_lossy().into_owned();
        }
    }
    (outcome, originals)
}

/// Plan the per-language, per-tool batches.
///
/// "Per-language, per-tool" because the same tool can be configured for two
/// languages, and the spec list lives on the language, not globally. A
/// tool that appears in two languages' specs is run twice, once per
/// language, so the bins are disjoint.
fn plan_tasks(work: &Work, root: &Path) -> Vec<PlannedTask> {
    // The bucketing itself is `languages::group_by_language`, so `doctor` and
    // `check` cannot disagree about which languages a repository contains -
    // doctor's whole job is to predict what check will do. What stays here is
    // only the part specific to this layer: reading one path per file out of
    // its hunks, and fanning each bucket out across that language's tools.
    //
    // `lint_only` is folded in alongside. A file too large for the LLM still
    // has a path, and these tools read the file themselves - excluding it
    // would silence ruff on a file purely because the model could not read it.
    let paths: Vec<&Path> = work
        .by_file
        .iter()
        .filter_map(|hunks| hunks.first())
        .map(|hunk| hunk.file_path.as_path())
        .chain(work.lint_only.iter().map(PathBuf::as_path))
        .collect();

    let repository_root = runner::absolute(root);
    languages::group_by_language(&paths)
        .into_iter()
        .flat_map(|(language, files)| {
            language.tools.iter().flat_map({
                let repository_root = repository_root.clone();
                move |spec| {
                    let mut workspaces: BTreeMap<PathBuf, Vec<PlannedFile>> = BTreeMap::new();
                    for file in &files {
                        let Some(workspace_root) =
                            runner::configuration_root(spec, &repository_root, file)
                        else {
                            continue;
                        };
                        let absolute = if file.is_absolute() {
                            (*file).to_path_buf()
                        } else {
                            repository_root.join(file)
                        };
                        let Ok(relative) = absolute.strip_prefix(&workspace_root) else {
                            continue;
                        };
                        let argument = relative.to_string_lossy().into_owned();
                        workspaces
                            .entry(workspace_root)
                            .or_default()
                            .push(PlannedFile {
                                original: (*file).to_path_buf(),
                                absolute,
                                argument,
                            });
                    }
                    workspaces
                        .into_iter()
                        .map(move |(workspace_root, files)| PlannedTask {
                            spec,
                            workspace_root,
                            files,
                        })
                }
            })
        })
        .collect()
}

/// Apply one tool outcome: append findings, and — for `Unavailable` —
/// record the per-file failure the orchestrator's exit-2 contract rests on.
fn merge_outcome(
    outcome: runner::ToolOutcome,
    files: Vec<PathBuf>,
    failures: &mut BTreeMap<PathBuf, FailureReason>,
    findings: &mut Vec<Finding>,
    compiled: &mut BTreeSet<PathBuf>,
) {
    if outcome.compilation_succeeded {
        compiled.extend(files.iter().cloned());
    }
    match outcome.status {
        runner::ToolStatus::Ok => {
            findings.extend(outcome.findings);
        }
        runner::ToolStatus::Skipped => {
            // The project has no opinion here. Nothing to do.
        }
        runner::ToolStatus::Unavailable => {
            let reason = FailureReason::ToolUnavailable {
                tool: outcome.tool.to_owned(),
                detail: outcome.detail,
            };
            let batch: BTreeMap<PathBuf, FailureReason> = files
                .into_iter()
                .map(|file| (file, reason.clone()))
                .collect();
            union_failures(failures, batch);
        }
    }
}
