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
use crate::languages::spec::{LanguageSupport, ToolSpec};

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
    // The language comes straight from `detect` and is carried in the value.
    // Keying on the name and then re-finding the language with a linear scan
    // over `all_languages()` meant the CLI re-deriving language identity from
    // a string, which `languages/` owns - and it left an `else { continue }`
    // arm unreachable by construction, so nothing could ever test it.
    //
    // The value holds path strings, not hunks. It used to hold cloned
    // `Vec<Vec<Hunk>>` - a deep copy of every line of every file in the work
    // set - purely so the file path could be read back out of the first hunk
    // afterwards. The path was already in hand at the point of the clone.
    let mut by_lang: BTreeMap<&'static str, (&'static LanguageSupport, Vec<String>)> =
        BTreeMap::new();
    for hunks in &work.by_file {
        let Some(hunk) = hunks.first() else {
            continue;
        };
        let Some(language) = languages::detect(&hunk.file_path) else {
            continue;
        };
        by_lang
            .entry(language.name)
            .or_insert_with(|| (language, Vec::new()))
            .1
            .push(hunk.file_path.to_string_lossy().into_owned());
    }

    by_lang
        .into_values()
        .flat_map(|(language, files)| {
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
