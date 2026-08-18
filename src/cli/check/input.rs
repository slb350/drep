//! Input resolution for `drep check`.
//!
//! Three modes, mutually exclusive at the clap layer, all surface here as
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

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::analysis::result::FailureReason;
use crate::cli::check::CheckArgs;
use crate::diff;
use crate::diff::hunks::Hunk;
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
}

/// Resolve `args` against `root` into a [`Work`].
///
/// The three modes are a fold of file paths into the same in-memory shape:
/// the deterministic layer and the LLM layer both consume `Vec<Vec<Hunk>>`,
/// so resolving differently per mode would mean a parallel code path
/// downstream. The point is to pay the per-mode divergence once, here.
pub async fn resolve(args: &CheckArgs, root: &Path) -> Result<Work> {
    // The name-only queries used to run first as an error probe and have
    // their results discarded. They buy nothing: `staged_hunks` and
    // `hunks_since` go through the same `staged_diff`/`since_diff` helpers,
    // with the same `has_head` probe, the same dash-guard on the ref, and the
    // same error paths - so they fail on exactly the conditions the probe was
    // watching for. Each discarded call cost two `git` spawns (~37 ms
    // measured), paid on every pre-commit and pre-push run before any useful
    // work started.
    let hunks = if args.staged {
        diff::staged_hunks(root).await?
    } else if let Some(git_ref) = args.diff.as_deref() {
        diff::hunks_since(root, git_ref).await?
    } else {
        return resolve_paths(&args.paths, root);
    };
    Ok(Work {
        by_file: group_by_file(hunks),
        read_failures: BTreeMap::new(),
        lint_only: Vec::new(),
    })
}

/// Paths mode: walk the user's paths, read each file, build a whole-file
/// hunk. I/O and UTF-8 errors land in `read_failures` rather than being
/// swallowed, so a file drep declined to analyze reaches the gate as a
/// failure rather than a reported-clean.
fn resolve_paths(paths: &[PathBuf], root: &Path) -> Result<Work> {
    let targets = resolve_paths_targets(paths, root);
    let mut by_file: Vec<Vec<Hunk>> = Vec::new();
    let mut read_failures: BTreeMap<PathBuf, FailureReason> = BTreeMap::new();
    let mut lint_only: Vec<PathBuf> = Vec::new();

    // `expand_paths` silently skips a path that does not exist - a deliberate
    // contract inherited from 1.x's `scan`, where a stale argument was not
    // worth failing over. Under this gate it is: `drep check typo.rs` would
    // resolve to zero targets and print "No issues found." A path the user
    // named and drep did not analyze is the same category as one too large to
    // send. Only *explicit* arguments are checked; a directory walk that finds
    // nothing is a legitimately empty result.
    for named in paths {
        if !named.exists() {
            read_failures.insert(
                named.clone(),
                FailureReason::Unreadable("no such file or directory".to_owned()),
            );
        }
    }
    for path in targets {
        let bytes = match std::fs::metadata(&path) {
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
        let content = match std::fs::read_to_string(&path) {
            Ok(content) => content,
            Err(err) => {
                read_failures.insert(path.clone(), FailureReason::Unreadable(err.to_string()));
                continue;
            }
        };
        by_file.push(vec![Hunk::whole_file(path, &content)]);
    }
    Ok(Work {
        by_file,
        read_failures,
        lint_only,
    })
}

/// Distinguish "the user passed a path" from "the user passed nothing".
///
/// `PATHS` (or nothing) → `["."]`. The CLI's mutual-exclusion rules leave
/// `paths` empty when `--staged` or `--diff` is set.
fn resolve_paths_targets(paths: &[PathBuf], root: &Path) -> Vec<PathBuf> {
    let predicate: fn(&Path) -> bool = files::is_scan_target;
    // Both branches go through `expand_paths`. The empty case used to return
    // the root path unexpanded, which is a *directory*: `metadata` succeeded,
    // the size gate passed, and `read_to_string` then failed with "Is a
    // directory". So bare `drep check` - the plainest invocation of the
    // primary command - reported the repo root as unreadable and exited 2
    // without analyzing anything. A special case that skips the expander is
    // how that happens; there is only one path now.
    if paths.is_empty() {
        return files::expand_paths(&[root.to_path_buf()], predicate);
    }
    files::expand_paths(paths, predicate)
}

/// Group hunks into `Vec<Vec<Hunk>>` keyed by file.
///
/// A `BTreeMap<PathBuf, Vec<Hunk>>` collected into values keeps the order
/// deterministic across runs of the same diff - the analyzer builds its
/// cache key from the file path, not from a list index, but a stable order
/// means the JSON output is stable too.
fn group_by_file(hunks: Vec<Hunk>) -> Vec<Vec<Hunk>> {
    let mut by_file: BTreeMap<PathBuf, Vec<Hunk>> = BTreeMap::new();
    for hunk in hunks {
        by_file
            .entry(hunk.file_path.clone())
            .or_default()
            .push(hunk);
    }
    by_file.into_values().collect()
}
