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

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use futures::stream::{self, StreamExt};

use crate::analysis::findings::Finding;
use crate::analysis::result::FailureReason;
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
    // In staged and branch modes a linter reads the working tree, so a task
    // that would read content the reviewed commit does not hold is refused
    // rather than run: each of its files fails with the differing paths named,
    // and the commit is not waved through on a lint of code it does not
    // contain. With no uncommitted paths - the ordinary case, and paths mode -
    // this changes nothing.
    let base = runner::absolute(root);
    let differences = Differences {
        differing: differing_paths(&base, &work.uncommitted),
        index_only: differing_paths(&base, &work.index_only),
    };
    let (tasks, refused) = partition_uncommitted(tasks, &base, &differences);
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
    fail_uncommitted_markers(unconfigured, &base, &differences, &mut failures);
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
pub(super) struct PlannedTask {
    pub(super) spec: &'static ToolSpec,
    pub(super) workspace_root: PathBuf,
    pub(super) files: Vec<PlannedFile>,
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

pub(super) struct PlannedFile {
    pub(super) original: PathBuf,
    pub(super) absolute: PathBuf,
    pub(super) argument: String,
}

/// Run one planned task and return its outcome alongside the files it was
/// given. The files list is moved back out so the caller can map the
/// outcome back to per-file failures without re-borrowing.
pub(super) async fn run_one(task: PlannedTask, root: &Path) -> (runner::ToolOutcome, Vec<PathBuf>) {
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
    // A file the build excludes leaves the batch: no tool looked at it, so it can neither fail nor count as compiled.
    let excluded: BTreeSet<&String> = outcome.excluded.iter().collect();
    let originals: Vec<PathBuf> = arguments
        .iter()
        .zip(originals)
        .filter(|(argument, _)| !excluded.contains(argument))
        .map(|(_, original)| original)
        .collect();
    // The canonical index is the second spelling `retain_requested` also
    // compares: a tool deriving paths from its own resolved cwd answers
    // through symlinks drep left alone. It is built on the first finding that
    // misses in exact form, because under an ordinary checkout none ever does
    // and resolving every planned file up front spends a `realpath` each to
    // answer a question nothing asks.
    let mut original_by_canonical: Option<BTreeMap<PathBuf, PathBuf>> = None;
    let mut root_prefix: Option<PathBuf> = None;
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
        finding.file_path = match original {
            Some(original) => original.to_string_lossy().into_owned(),
            // A type-check failure can name a file nobody asked about. The tool spelled it from the workspace, which `configuration_root` built from the absolute, lexically normal root, so it is respelled from that same root, where the caller's own paths start.
            None => {
                let normal = runner::lexically_normal(&joined);
                let prefix = root_prefix
                    .get_or_insert_with(|| runner::lexically_normal(&runner::absolute(root)));
                normal
                    .strip_prefix(prefix)
                    .unwrap_or(&normal)
                    .to_string_lossy()
                    .into_owned()
            }
        };
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
    canonical: OnceLock<Option<PathBuf>>,
}

impl Differing {
    fn canonical(&self) -> Option<&PathBuf> {
        self.canonical
            .get_or_init(|| self.absolute.canonicalize().ok())
            .as_ref()
    }
}

/// What the working tree does not hold of the commit: paths it holds
/// otherwise (`differing`), and configuration a sparse checkout left out of
/// it (`index_only`), which no tool can read but which still configures one.
struct Differences {
    differing: Vec<Differing>,
    index_only: Vec<Differing>,
}

impl Differences {
    /// The paths among both that name one of `spec`'s markers in
    /// `directories`, directly or through a link, sorted.
    fn markers(&self, spec: &ToolSpec, directories: &BTreeSet<PathBuf>) -> Vec<PathBuf> {
        markers_in(&[&self.differing, &self.index_only], spec, directories)
    }

    /// The paths among `index_only` alone that name one of `spec`'s markers in
    /// `directories`. Used when every `differing` path already refuses the task.
    fn index_only_markers(&self, spec: &ToolSpec, directories: &BTreeSet<PathBuf>) -> Vec<PathBuf> {
        markers_in(&[&self.index_only], spec, directories)
    }
}

/// The paths across `slices` that name one of `spec`'s markers in
/// `directories`, directly or through a link, sorted.
fn markers_in(
    slices: &[&[Differing]],
    spec: &ToolSpec,
    directories: &BTreeSet<PathBuf>,
) -> Vec<PathBuf> {
    if slices.iter().all(|slice| slice.is_empty()) {
        return Vec::new();
    }
    let links = marker_links(spec, directories);
    let mut paths: Vec<PathBuf> = slices
        .iter()
        .flat_map(|slice| {
            named_where(slice, |path| {
                names_marker(spec, directories, &path.absolute) || links.contains(&path.absolute)
            })
        })
        .collect();
    paths.sort();
    paths
}

/// The uncommitted paths in the order `uncommitted` holds them, which is
/// sorted, so every subset taken from them is too.
fn differing_paths(base: &Path, uncommitted: &BTreeSet<PathBuf>) -> Vec<Differing> {
    uncommitted
        .iter()
        .map(|named| Differing {
            absolute: runner::lexically_normal(&base.join(named)),
            named: named.clone(),
            canonical: OnceLock::new(),
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
    differences: &Differences,
) -> (Vec<PlannedTask>, Vec<(PlannedTask, Vec<PathBuf>)>) {
    if differences.differing.is_empty() && differences.index_only.is_empty() {
        return (tasks, Vec::new());
    }
    let mut runnable = Vec::new();
    let mut refused = Vec::new();
    for task in tasks {
        let reads = task_uncommitted_reads(&task, base, differences);
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
    differences: &Differences,
) -> Vec<PathBuf> {
    let directories =
        marker_directories(task.files.iter().map(|file| file.absolute.as_path()), base);
    let files: HashSet<PathBuf> = task
        .files
        .iter()
        .map(|file| runner::lexically_normal(&file.absolute))
        .collect();
    // A marker a sparse checkout left out is still the commit's configuration,
    // and a batch file it left out is one the tool would never see, however it
    // reads, while the task would report that file checked.
    let mut reads = if task.spec.reads_other_sources {
        let mut reads = differences.index_only_markers(task.spec, &directories);
        reads.extend(named_where(&differences.differing, |_| true));
        reads
    } else {
        let mut reads = differences.markers(task.spec, &directories);
        if !differences.differing.is_empty() {
            let targets = canonical_reads(task, &directories);
            reads.extend(named_where(&differences.differing, |path| {
                files.contains(&path.absolute)
                    || path
                        .canonical()
                        .is_some_and(|target| targets.contains(target))
            }));
        }
        reads
    };
    reads.extend(named_where(&differences.index_only, |path| {
        files.contains(&path.absolute)
    }));
    reads.sort();
    reads.dedup();
    reads
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
    differences: &Differences,
    failures: &mut BTreeMap<PathBuf, FailureReason>,
) {
    if differences.differing.is_empty() && differences.index_only.is_empty() {
        return;
    }
    // Group by `(tool, parent directory)` so marker-symlink resolution runs
    // once per directory rather than once per unconfigured file.
    let mut groups: Vec<(&'static ToolSpec, Option<PathBuf>, Vec<PathBuf>)> = Vec::new();
    let mut by_key: HashMap<(*const ToolSpec, Option<PathBuf>), usize> = HashMap::new();
    for pair in unconfigured {
        let parent = pair.absolute.parent().map(Path::to_path_buf);
        let key = (std::ptr::from_ref(pair.spec), parent);
        if let Some(&idx) = by_key.get(&key) {
            groups[idx].2.push(pair.file);
        } else {
            let idx = groups.len();
            let parent_dir = key.1.clone();
            by_key.insert(key, idx);
            groups.push((pair.spec, parent_dir, vec![pair.file]));
        }
    }
    for (spec, parent, files) in groups {
        let directories = parent
            .as_deref()
            .map(|dir| runner::ancestors_within(dir, base).into_iter().collect())
            .unwrap_or_default();
        let paths = differences.markers(spec, &directories);
        if !paths.is_empty() {
            let reason = FailureReason::UncommittedChanges {
                tool: spec.name.to_owned(),
                paths,
            };
            fail_all(files, &reason, failures);
        }
    }
}

/// Plan the per-tool, per-workspace batches, alongside the (tool, file)
/// pairs that found no configured workspace.
///
/// Keyed by `(tool, workspace_root)` rather than per-language: a tool shared by
/// multiple languages in the same workspace (such as `eslint` across JS/TS/Vue/
/// Svelte or `cppcheck` across C/C++) spawns once with all matching files
/// instead of paying repeated cold starts.
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
    let mut config_cache = runner::ConfigCache::new(&repository_root);
    let mut tasks: Vec<PlannedTask> = Vec::new();
    let mut task_index: HashMap<(*const ToolSpec, PathBuf), usize> = HashMap::new();
    let mut unconfigured = Vec::new();
    for (language, files) in languages::group_by_language(&paths) {
        for &spec in language.tools {
            for file in &files {
                let absolute = if file.is_absolute() {
                    (*file).to_path_buf()
                } else {
                    repository_root.join(file)
                };
                let Some(workspace_root) = config_cache.configuration_root(spec, file) else {
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
                let planned = PlannedFile {
                    original: (*file).to_path_buf(),
                    absolute,
                    argument,
                };
                let key = (std::ptr::from_ref(spec), workspace_root);
                if let Some(&idx) = task_index.get(&key) {
                    tasks[idx].files.push(planned);
                } else {
                    let idx = tasks.len();
                    let workspace_root = key.1.clone();
                    task_index.insert(key, idx);
                    tasks.push(PlannedTask {
                        spec,
                        workspace_root,
                        files: vec![planned],
                    });
                }
            }
        }
    }
    (tasks, unconfigured)
}

/// Apply one tool outcome: append findings, and — for `Unavailable` —
/// record the per-file failure the orchestrator's exit-2 contract rests on.
pub(super) fn merge_outcome(
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
    for file in files {
        failures.entry(file).or_insert_with(|| reason.clone());
    }
}
