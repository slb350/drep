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
    let (tasks, unconfigured) = plan_tasks(work, root);
    // In staged mode a linter reads the working tree, so a task that would
    // read content the commit does not hold is refused rather than run: each
    // of its files fails with the differing paths named, and the commit is
    // not waved through on a lint of code it does not contain. With no
    // uncommitted paths - the ordinary case, and every other mode - this
    // changes nothing.
    let base = runner::absolute(root);
    let differing = differing_paths(&base, &work.uncommitted);
    let (tasks, refused) = partition_uncommitted(tasks, &base, &differing);
    for (task, paths) in refused {
        let reason = FailureReason::UncommittedChanges {
            tool: task.spec.name.to_owned(),
            paths,
        };
        fail_all(
            task.files.into_iter().map(|file| file.original),
            &reason,
            &mut failures,
        );
    }
    fail_uncommitted_markers(unconfigured, &base, &differing, &mut failures);
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
/// invoked with. The tool name is on the spec, so it is not duplicated here.
struct PlannedTask {
    spec: &'static ToolSpec,
    workspace_root: PathBuf,
    files: Vec<PlannedFile>,
}

/// A (tool, file) pair whose tool found no configured workspace on disk.
///
/// Ordinarily dropped - an unconfigured project has not opted into the tool.
/// Kept here because in staged mode the marker may be missing from the
/// working tree while the committing index still holds it, and silently
/// skipping the file then reports as clean a file the commit's own
/// configuration would have linted.
struct Unconfigured {
    spec: &'static ToolSpec,
    /// The file as the caller named it, which the failure reports.
    file: PathBuf,
    /// The same file absolute, which the marker comparison resolves against.
    absolute: PathBuf,
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
        ..
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

/// One uncommitted path, as the user names it (which a failure reports) and
/// as a lexically normal absolute path (which the comparisons use). Named
/// from a subdirectory, an entry climbs with `..`, which a plain join leaves
/// in place.
///
/// Comparisons are lexical on absolute paths, never canonicalized: a symlinked
/// checkout is a spelling the rest of this module deliberately leaves alone,
/// and `realpath` would answer about a different path than the one the tool
/// opens.
struct Differing {
    absolute: PathBuf,
    named: PathBuf,
}

/// The uncommitted paths in the order `uncommitted` holds them, which is
/// sorted, so every subset taken from them is too.
fn differing_paths(base: &Path, uncommitted: &BTreeSet<PathBuf>) -> Vec<Differing> {
    uncommitted
        .iter()
        .map(|named| Differing {
            absolute: runner::lexically_normal(&base.join(named)),
            named: named.clone(),
        })
        .collect()
}

/// The paths among `differing` that `reads` accepts, as the user names them,
/// sorted.
fn named_where(differing: &[Differing], reads: impl Fn(&Differing) -> bool) -> Vec<PathBuf> {
    differing
        .iter()
        .filter(|path| reads(path))
        .map(|path| path.named.clone())
        .collect()
}

/// Split `tasks` into those that may run and those that would read
/// working-tree content the commit does not hold, with the differing paths
/// each refused task would read.
fn partition_uncommitted(
    tasks: Vec<PlannedTask>,
    base: &Path,
    differing: &[Differing],
) -> (Vec<PlannedTask>, Vec<(PlannedTask, Vec<PathBuf>)>) {
    if differing.is_empty() {
        return (tasks, Vec::new());
    }
    let mut runnable = Vec::new();
    let mut refused = Vec::new();
    for task in tasks {
        let reads = task_uncommitted_reads(&task, base, differing);
        if reads.is_empty() {
            runnable.push(task);
        } else {
            refused.push((task, reads));
        }
    }
    (runnable, refused)
}

/// The differing paths `task` would read, as the user names them, sorted.
///
/// A tool that reads beyond the files it is given (`ToolSpec::reads_other_sources`:
/// every whole-project tool, and ShellCheck following `source`, eslint and
/// cppcheck following imports and includes) is refused by any uncommitted
/// path: a path dependency, an import or a sourced helper can sit anywhere in
/// the repository, under any name, and in another language drep registers (a
/// C++ file includes a C header, TypeScript imports JavaScript). A tool that
/// reads only its files reads its batch and its config markers, through any
/// symlink, in any directory from a batch file's own up to the root: the workspace is the
/// nearest configured one on disk, and a nearer marker the working tree no
/// longer holds can be the commit's configuration.
fn task_uncommitted_reads(
    task: &PlannedTask,
    base: &Path,
    differing: &[Differing],
) -> Vec<PathBuf> {
    if task.spec.reads_other_sources {
        return named_where(differing, |_| true);
    }
    let files: Vec<PathBuf> = task
        .files
        .iter()
        .map(|file| runner::lexically_normal(&file.absolute))
        .collect();
    let directories =
        marker_directories(task.files.iter().map(|file| file.absolute.as_path()), base);
    let targets = canonical_reads(task, &directories);
    let links = marker_links(task.spec, &directories);
    named_where(differing, |path| {
        files.contains(&path.absolute)
            || names_marker(task.spec, &directories, &path.absolute)
            || links.contains(&path.absolute)
            || path
                .absolute
                .canonicalize()
                .is_ok_and(|target| targets.contains(&target))
    })
}

/// Where each of `spec`'s markers in `directories` that is a symlink points,
/// followed lexically link by link, so a link whose target the working tree
/// no longer holds still names it: git reports the deleted target, not the
/// unchanged link, and the tool, finding no marker on disk, is not configured
/// by it. A glob marker names no one file to follow.
fn marker_links(spec: &ToolSpec, directories: &BTreeSet<PathBuf>) -> BTreeSet<PathBuf> {
    let mut targets = BTreeSet::new();
    for directory in directories {
        for name in spec
            .config_files
            .iter()
            .filter(|name| !name.starts_with("*."))
        {
            let mut link = directory.join(name);
            // A target already found ends the chain, so a cycle cannot loop.
            while let Ok(target) = std::fs::read_link(&link) {
                let next = runner::lexically_normal(&match link.parent() {
                    Some(parent) => parent.join(&target),
                    None => target,
                });
                if !targets.insert(next.clone()) {
                    break;
                }
                link = next;
            }
        }
    }
    targets
}

/// What a tool that reads only its files opens, followed through any symlink:
/// its batch files and the config marker it finds in each of `directories`.
/// Git names a symlink's target when the target differs, not the link a batch
/// holds, so a differing path is matched by its canonical target too, as the
/// findings a tool reports through a symlinked checkout are.
fn canonical_reads(task: &PlannedTask, directories: &BTreeSet<PathBuf>) -> BTreeSet<PathBuf> {
    let markers = directories.iter().filter_map(|directory| {
        runner::configured_marker(task.spec, directory).map(|name| directory.join(name))
    });
    task.files
        .iter()
        .map(|file| file.absolute.clone())
        .chain(markers)
        .filter_map(|path| path.canonicalize().ok())
        .collect()
}

/// Every directory a tool's config markers for `files` can be rooted in: each
/// file's own directory and every one above it within `base`.
fn marker_directories<'a>(files: impl Iterator<Item = &'a Path>, base: &Path) -> BTreeSet<PathBuf> {
    files
        .filter_map(Path::parent)
        .flat_map(|directory| runner::ancestors_within(directory, base))
        .collect()
}

/// Whether `path` is one of `spec`'s config markers rooted in one of
/// `directories`. A marker can name a nested path (Checkstyle's
/// `config/checkstyle/checkstyle.xml`), so each directory is tried as its
/// root rather than only the marker's own parent.
fn names_marker(spec: &ToolSpec, directories: &BTreeSet<PathBuf>, path: &Path) -> bool {
    directories.iter().any(|directory| {
        path.starts_with(directory)
            && spec
                .config_files
                .iter()
                .any(|name| runner::marker_names_path(directory, name, path))
    })
}

/// Fail the file of each pair whose tool found no configured workspace when
/// one of the tool's config markers is uncommitted in the file's directory or
/// a directory above it within the root.
///
/// Such a pair is ordinarily dropped: the project has not opted in. But in
/// staged mode the marker can be absent from the working tree while the
/// committing index still holds it, and then the tool that should have gated
/// the file would be silently skipped.
fn fail_uncommitted_markers(
    unconfigured: Vec<Unconfigured>,
    base: &Path,
    differing: &[Differing],
    failures: &mut BTreeMap<PathBuf, FailureReason>,
) {
    if differing.is_empty() {
        return;
    }
    for pair in unconfigured {
        let directories = marker_directories(std::iter::once(pair.absolute.as_path()), base);
        let links = marker_links(pair.spec, &directories);
        let paths = named_where(differing, |path| {
            names_marker(pair.spec, &directories, &path.absolute) || links.contains(&path.absolute)
        });
        if !paths.is_empty() {
            let reason = FailureReason::UncommittedChanges {
                tool: pair.spec.name.to_owned(),
                paths,
            };
            fail_all([pair.file], &reason, failures);
        }
    }
}

/// Plan the per-language, per-tool batches, alongside the (tool, file)
/// pairs that found no configured workspace.
///
/// "Per-language, per-tool" because the same tool can be configured for two
/// languages, and the spec list lives on the language, not globally. A
/// tool that appears in two languages' specs is run twice, once per
/// language, so the bins are disjoint.
///
/// An unconfigured pair is recorded rather than discarded: in staged mode
/// the marker may be missing from the working tree while the committing
/// index holds it, and the caller needs the pair to tell "the project never
/// opted in" from "the commit opted in and the working tree disagrees".
fn plan_tasks(work: &Work, root: &Path) -> (Vec<PlannedTask>, Vec<Unconfigured>) {
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
    let mut tasks = Vec::new();
    let mut unconfigured = Vec::new();
    for (language, files) in languages::group_by_language(&paths) {
        for spec in language.tools {
            let mut workspaces: BTreeMap<PathBuf, Vec<PlannedFile>> = BTreeMap::new();
            for file in &files {
                let absolute = if file.is_absolute() {
                    (*file).to_path_buf()
                } else {
                    repository_root.join(file)
                };
                let Some(workspace_root) = runner::configuration_root(spec, &repository_root, file)
                else {
                    unconfigured.push(Unconfigured {
                        spec,
                        file: (*file).to_path_buf(),
                        absolute,
                    });
                    continue;
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
            tasks.extend(
                workspaces
                    .into_iter()
                    .map(|(workspace_root, files)| PlannedTask {
                        spec,
                        workspace_root,
                        files,
                    }),
            );
        }
    }
    (tasks, unconfigured)
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
            fail_all(files, &reason, failures);
        }
    }
}

/// Record `reason` for every one of `files`, the first reason winning on a collision as everywhere in the orchestrator.
fn fail_all(
    files: impl IntoIterator<Item = PathBuf>,
    reason: &FailureReason,
    failures: &mut BTreeMap<PathBuf, FailureReason>,
) {
    let batch: BTreeMap<PathBuf, FailureReason> = files
        .into_iter()
        .map(|file| (file, reason.clone()))
        .collect();
    union_failures(failures, batch);
}
