//! Input resolution for `drep check`.
//!
//! Four modes, mutually exclusive at the clap layer, all surface here as
//! one [`Work`] value: a list of files, a list of hunks per file, and a
//! per-file map of failure reasons for the things that could not be read
//! (oversize, non-UTF-8, missing). The orchestrator downstream does not need
//! to know which mode produced the work.
//!
//! Two invariants this module owns:
//!
//! - "No files changed" is **distinct** from "I could not ask git". A git
//!   failure in either diff mode is a hard error, not an empty work set.
//! - A file drep decided not to analyze (oversize, unreadable) is a
//!   *failure*, not a skip. The orchestrator's exit-2 contract depends on
//!   every file either getting a hunk or getting a `FailureReason`.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use anyhow::{Result, bail};

use crate::analysis::result::FailureReason;
use crate::cli::check::CheckArgs;
use crate::diff;
use crate::diff::hunks::{Hunk, group_by_file};
use crate::files;

/// The largest file drep will read into memory in paths mode.
///
/// Deliberately generous and deliberately *not*
/// [`crate::analysis::payload::PAYLOAD_MAX_BYTES`]: this guards against an
/// `OutOfMemory` on a pathological file, while the payload ceiling guards the
/// size of the request sent to the model. They were the same constant, which
/// made a file's reported size depend on which code path measured it.
///
/// Any value at or above the payload ceiling is safe, because a payload is
/// never smaller than the file it renders. 8 MiB is far enough above it that
/// this guard effectively only fires on files nobody meant to review, leaving
/// the payload check as the single authority on "too large to analyze".
pub const READ_MAX_BYTES: u64 = 8 * 1024 * 1024;

/// The read guard must never sit below the payload ceiling.
///
/// If it did, paths mode would reject a file whose rendered payload would have
/// been accepted, and the two checks would disagree about the same file. A
/// `const` assertion rather than a test: this is a property of the two
/// literals, so it should fail the build, not a test run.
const _: () = assert!(READ_MAX_BYTES >= crate::analysis::payload::PAYLOAD_MAX_BYTES);

/// The work set for one `check` run: every file drep will consider, the
/// hunks per file, and the files that could not be read for one of the
/// well-defined reasons (rather than silently dropped).
pub struct Work {
    /// One entry per file, each holding that file's hunks.
    ///
    /// Ordered, not keyed: in the diff modes `group_by_file` builds it from a
    /// `BTreeMap` so repeated runs over the same diff produce the same order,
    /// and in paths mode the order is `expand_paths`' (itself a `BTreeSet`).
    /// Either way the JSON output is stable across runs.
    pub by_file: Vec<Vec<Hunk>>,
    /// Files that could not be read off disk or were rejected by the
    /// read guard. Unioned into the orchestrator's failure map.
    pub read_failures: BTreeMap<PathBuf, FailureReason>,
    /// Files drep knows about but holds no content for.
    ///
    /// A file too large to read still has a path, and ruff/clippy/go vet take
    /// paths - they read the file themselves. Dropping it from `by_file`
    /// therefore silenced the deterministic layer as a side effect of an LLM
    /// size limit: `drep check big.py` reported no ruff findings for it, while
    /// `drep check --staged` on the same file reported them, because the diff
    /// modes never consulted the read guard. The file is still a failure (the
    /// LLM never saw it), but its linters keep running.
    pub lint_only: Vec<PathBuf>,
    /// Directories whose repositories contain the bytes semantic review would
    /// send.
    ///
    /// Paths mode records the canonical target directory after resolving an
    /// explicit symlink and reads from that same canonical path. Diff modes
    /// record the repository-relative hunk locations because deleted files may
    /// no longer exist on disk. Keeping this beside the work set makes policy
    /// scope a property of the bytes that were resolved, not a second walk over
    /// display paths that can point somewhere else. It remains empty when the
    /// loaded site policy names no refusal markers, avoiding path allocations on
    /// unaffected machines.
    pub(super) reviewed_directories: BTreeSet<PathBuf>,
}

/// The pushed range pre-commit derived from git's pre-push stdin.
///
/// Existing remote refs produce an exact range. For a new branch whose pushed
/// history reaches a root commit, pre-commit deliberately selects all files and
/// omits its FROM/TO variables; [`AllFiles`](Self::AllFiles) preserves that
/// distinct decision rather than inventing a base.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PreCommitPush {
    Range { from: String, to: String },
    AllFiles,
}

impl PreCommitPush {
    /// Read the environment pre-commit exposes to a pre-push hook.
    fn from_env() -> Result<Self> {
        Self::from_lookup(|name| std::env::var_os(name))
    }

    /// Parse through an injected lookup so tests never mutate process-global
    /// environment while the Rust suite runs concurrently.
    pub(crate) fn from_lookup(mut lookup: impl FnMut(&str) -> Option<OsString>) -> Result<Self> {
        let mut value = |name: &str| {
            lookup(name)
                .map(|raw| {
                    raw.into_string().map_err(|_| {
                        anyhow::anyhow!("pre-commit environment variable `{name}` is not UTF-8")
                    })
                })
                .transpose()
        };

        if value("PRE_COMMIT")?.as_deref() != Some("1") {
            bail!("--pre-commit-push must be invoked by pre-commit's pre-push hook");
        }
        // PRE_COMMIT_FROM_REF/TO_REF are the current names. ORIGIN/SOURCE are
        // their legacy aliases and keep the published hook compatible with
        // older pre-commit installations.
        let from = value("PRE_COMMIT_FROM_REF")?.or(value("PRE_COMMIT_ORIGIN")?);
        let to = value("PRE_COMMIT_TO_REF")?.or(value("PRE_COMMIT_SOURCE")?);
        match (from, to) {
            (Some(from), Some(to)) if !from.is_empty() && !to.is_empty() => {
                Ok(Self::Range { from, to })
            }
            (None, None) => {
                let remote = value("PRE_COMMIT_REMOTE_NAME")?;
                if remote.as_deref().is_none_or(str::is_empty) {
                    bail!(
                        "--pre-commit-push has no ref range or `PRE_COMMIT_REMOTE_NAME`; the hook context is incomplete"
                    );
                }
                Ok(Self::AllFiles)
            }
            _ => bail!(
                "--pre-commit-push requires both `PRE_COMMIT_FROM_REF` and `PRE_COMMIT_TO_REF`, or neither for an all-files new-branch push"
            ),
        }
    }
}

/// Resolve `args` against `root` into a [`Work`].
///
/// The four modes are a fold of file paths into the same in-memory shape:
/// the deterministic layer and the LLM layer both consume `Vec<Vec<Hunk>>`,
/// so resolving differently per mode would mean a parallel code path
/// downstream. The point is to pay the per-mode divergence once, here.
/// `collect_policy_scope` is true only when site policy names refusal markers;
/// the source-location bookkeeping is otherwise unnecessary.
pub async fn resolve(args: &CheckArgs, root: &Path, collect_policy_scope: bool) -> Result<Work> {
    if args.pre_commit_push {
        return resolve_pre_commit(root, &PreCommitPush::from_env()?, collect_policy_scope).await;
    }

    let hunks = if args.staged {
        diff::staged_hunks(root, files::is_scan_target).await?
    } else if let Some(git_ref) = args.diff.as_deref() {
        diff::hunks_between(root, git_ref, args.tip.as_deref(), files::is_scan_target).await?
    } else {
        return resolve_paths(&args.paths, root, collect_policy_scope);
    };
    let by_file = group_by_file(hunks);
    Ok(Work {
        reviewed_directories: hunk_directories(root, &by_file, collect_policy_scope),
        by_file,
        read_failures: BTreeMap::new(),
        lint_only: Vec::new(),
    })
}

/// Resolve an already-parsed pre-commit context.
///
/// The seam keeps environment access at the production boundary and lets the
/// tests prove range and all-files behavior without racing on `set_var`.
pub(crate) async fn resolve_pre_commit(
    root: &Path,
    pre_commit: &PreCommitPush,
    collect_policy_scope: bool,
) -> Result<Work> {
    // The name-only queries used to run first as an error probe and have
    // their results discarded. They buy nothing: `staged_hunks` and
    // `hunks_since` go through the same `staged_diff`/`since_diff` helpers,
    // with the same `has_head` probe, the same dash-guard on the ref, and the
    // same error paths - so they fail on exactly the conditions the probe was
    // watching for. Each discarded call cost two `git` spawns (~37 ms
    // measured), paid on every pre-commit and pre-push run before any useful
    // work started.
    let hunks = match pre_commit {
        PreCommitPush::Range { from, to } => {
            diff::hunks_between(root, from, Some(to), files::is_scan_target).await?
        }
        PreCommitPush::AllFiles => return resolve_paths(&[], root, collect_policy_scope),
    };
    let by_file = group_by_file(hunks);
    Ok(Work {
        reviewed_directories: hunk_directories(root, &by_file, collect_policy_scope),
        by_file,
        read_failures: BTreeMap::new(),
        lint_only: Vec::new(),
    })
}

/// Paths mode: walk the user's paths, read each file, build a whole-file
/// hunk. I/O and UTF-8 errors land in `read_failures` rather than being
/// swallowed, so a file drep declined to analyze reaches the gate as a
/// failure rather than a reported-clean.
fn resolve_paths(paths: &[PathBuf], root: &Path, collect_policy_scope: bool) -> Result<Work> {
    // `files::expand_named` owns the whole "what did the user ask for, and what
    // could I not do with it" question, including the no-arguments default and
    // the named-path rejections. This function used to re-walk `paths` itself
    // to rediscover the rejections, and `lint-docs` grew a second copy of the
    // same loop; both missed a named path that exists but is neither a regular
    // file nor a directory.
    let files::Expansion { targets, rejected } =
        files::expand_named(paths, root, files::is_scan_target);

    let mut by_file: Vec<Vec<Hunk>> = Vec::new();
    let mut read_failures: BTreeMap<PathBuf, FailureReason> = BTreeMap::new();
    let mut lint_only: Vec<PathBuf> = Vec::new();
    let mut reviewed_directories: BTreeSet<PathBuf> = BTreeSet::new();

    for (path, why) in rejected {
        let reason = match why {
            files::Rejected::Missing => {
                FailureReason::Unreadable("no such file or directory".to_owned())
            }
            // The hint comes from `files::redirect_hint`, so `check` does not
            // hold its own opinion about which command handles markdown.
            files::Rejected::Unanalyzable => {
                FailureReason::unsupported(&path, files::redirect_hint(&path))
            }
        };
        read_failures.insert(path, reason);
    }

    for path in targets {
        // Resolve once, then use the resolved location for both the read and
        // policy scope. Reading through the lexical symlink and canonicalizing
        // later leaves a target-swap window where the two operations answer
        // about different source. The lexical path remains on the Hunk so
        // findings still name what the user typed.
        let source_path = match std::fs::canonicalize(&path) {
            Ok(path) => path,
            Err(err) => {
                read_failures.insert(path, FailureReason::Unreadable(err.to_string()));
                continue;
            }
        };
        let bytes = match std::fs::metadata(&source_path) {
            Ok(meta) => meta.len(),
            Err(err) => {
                read_failures.insert(path, FailureReason::Unreadable(err.to_string()));
                continue;
            }
        };
        // An I/O guard, not the model's ceiling. Its job is to stop
        // `read_to_string` pulling a pathological file into memory whole; the
        // authoritative size decision is made on the *rendered payload* in
        // `analysis::code_quality`, which every input mode passes through.
        //
        // The invariant that matters runs one way only: because a payload
        // contains its file's content verbatim, this guard can never *reject*
        // a file whose payload would have been accepted. It can and does
        // accept files the payload check then rejects - a 200 KB file renders
        // to more than 200 KB once the header and the ten-byte gutter per line
        // are added - and that is fine, because the number the user is shown
        // then comes from the check that actually measured it. An earlier
        // version of this comment claimed the opposite direction, which is
        // what hid the fact that the two checks were measuring different
        // things under one constant.
        if bytes > READ_MAX_BYTES {
            read_failures.insert(
                path.clone(),
                FailureReason::FileTooLarge {
                    bytes,
                    limit: READ_MAX_BYTES,
                },
            );
            // Still linted: see `Work::lint_only`.
            lint_only.push(path);
            continue;
        }
        let content = match std::fs::read_to_string(&source_path) {
            Ok(content) => content,
            Err(err) => {
                read_failures.insert(path.clone(), FailureReason::Unreadable(err.to_string()));
                continue;
            }
        };
        if collect_policy_scope && let Some(parent) = source_path.parent() {
            reviewed_directories.insert(parent.to_path_buf());
        }
        by_file.push(vec![Hunk::whole_file(path, &content)]);
    }
    Ok(Work {
        by_file,
        read_failures,
        lint_only,
        reviewed_directories,
    })
}

/// Repository-discovery starting points for diff-backed hunks.
///
/// These paths may name deleted files, so canonicalizing them would turn a
/// valid diff into an input failure. Git produced every hunk relative to
/// `root`; its lexical parent is therefore the directory whose repository must
/// answer the policy query.
fn hunk_directories(
    root: &Path,
    by_file: &[Vec<Hunk>],
    collect_policy_scope: bool,
) -> BTreeSet<PathBuf> {
    if !collect_policy_scope {
        return BTreeSet::new();
    }
    by_file
        .iter()
        .filter_map(|hunks| hunks.first())
        .filter_map(|hunk| root.join(&hunk.file_path).parent().map(Path::to_path_buf))
        .collect()
}
