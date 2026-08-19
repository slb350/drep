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

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use futures::future::join_all;

use crate::analysis::findings::Finding;
use crate::analysis::result::{FailureReason, union_failures};
use crate::cli::check::input::Work;
use crate::languages;
use crate::languages::runner::{self};
use crate::languages::spec::ToolSpec;

/// Run every configured deterministic tool against the work set and return
/// its findings.
///
/// `failures` is filled in place: every file in a batch whose tool ran with
/// `ToolStatus::Unavailable` lands there with a `FailureReason::ToolUnavailable`.
/// The orchestrator unions this with the LLM layer's failures; the first
/// reason wins on a key collision, matching `AnalysisResult::merge`.
pub async fn run(work: &Work, root: &Path) -> (Vec<Finding>, BTreeMap<PathBuf, FailureReason>) {
    let mut failures: BTreeMap<PathBuf, FailureReason> = BTreeMap::new();
    let tasks = plan_tasks(work);
    let futures = tasks
        .into_iter()
        .map(|task| async move { run_one(task, root).await });
    let outcomes = join_all(futures).await;
    let mut findings = Vec::new();
    for (outcome, files) in outcomes {
        merge_outcome(outcome, files, &mut failures, &mut findings);
    }
    (findings, failures)
}

/// One deterministic-tool invocation: the spec and the files it should be
/// invoked with. The tool name is on the spec, so it is not duplicated
/// here.
struct PlannedTask {
    spec: &'static ToolSpec,
    files: Vec<String>,
}

/// Run one planned task and return its outcome alongside the files it was
/// given. The files list is moved back out so the caller can map the
/// outcome back to per-file failures without re-borrowing.
async fn run_one(task: PlannedTask, root: &Path) -> (runner::ToolOutcome, Vec<String>) {
    let outcome = runner::run_tool(task.spec, root, &task.files).await;
    (outcome, task.files)
}

/// Plan the per-language, per-tool batches.
///
/// "Per-language, per-tool" because the same tool can be configured for two
/// languages, and the spec list lives on the language, not globally. A
/// tool that appears in two languages' specs is run twice, once per
/// language, so the bins are disjoint.
fn plan_tasks(work: &Work) -> Vec<PlannedTask> {
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

    languages::group_by_language(&paths)
        .into_iter()
        .flat_map(|(language, files)| {
            let files: Vec<String> = files
                .into_iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect();
            language.tools.iter().map(move |spec| PlannedTask {
                spec,
                files: files.clone(),
            })
        })
        .collect()
}

/// Apply one tool outcome: append findings, and — for `Unavailable` —
/// record the per-file failure the orchestrator's exit-2 contract rests on.
fn merge_outcome(
    outcome: runner::ToolOutcome,
    files: Vec<String>,
    failures: &mut BTreeMap<PathBuf, FailureReason>,
    findings: &mut Vec<Finding>,
) {
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
                .map(|file| (PathBuf::from(file), reason.clone()))
                .collect();
            union_failures(failures, batch);
        }
    }
}
