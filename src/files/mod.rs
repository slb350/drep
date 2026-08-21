//! The only file-target policy.
//!
//! Every code path in drep that decides which files to look at goes through
//! these predicates, so a Python file is a file is a file regardless of whether
//! it was discovered by a full scan, the staged-files index, or a
//! `--diff <ref>` query. Markdown has its own predicate and its own command:
//! see `is_scan_target` for why the two file classes are disjoint.
//!
//! Walking is the `ignore` crate rather than `std::fs::read_dir` so a project's
//! gitignore is honoured without a second pass, and vendored directories are
//! pruned during the walk (rather than collected-then-filtered, which would
//! `stat` every entry under `node_modules/` before discarding them).
//!
//! Discovery of explicit filenames (`drep check a.rs .`) deliberately ignores
//! gitignore: the user naming a path is a stronger signal than a repo-wide
//! pattern. `expand_paths` enforces that; `walk_targets` enforces the inverse.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use ignore::{DirEntry, WalkBuilder};

use crate::languages;

/// Any registered language's source file - the file class `drep check` reads.
///
/// Markdown is **not** here, and that is the point. Each command owns one file
/// class: `check` reads code through this predicate, while `lint-docs` reads
/// markdown through [`is_markdown`]. A path the user names that falls outside
/// the running command's class is a
/// [`crate::analysis::result::FailureReason::Unsupported`] pointing at the
/// other command - never a silent skip.
pub fn is_scan_target(path: &Path) -> bool {
    languages::detect(path).is_some()
}

/// Markdown document.
pub fn is_markdown(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
}

/// Hardcoded ignored dirs that belong to no single language: VCS metadata,
/// build output, caches.
const SHARED_IGNORED_DIRS: &[&str] = &[".git", "build", "dist", ".cache"];

/// A directory that should never be descended into.
///
/// Case-insensitive on purpose: a directory called `VENV` and one called `venv`
/// are the same on a case-insensitive filesystem, and matching only one of
/// them would walk it anyway — defeating the whole point of having the list.
pub fn is_ignored_dir(name: &str) -> bool {
    let folded = name.to_ascii_lowercase();
    if folded.ends_with(".egg-info") {
        return true;
    }
    if SHARED_IGNORED_DIRS.iter().any(|d| *d == folded) {
        return true;
    }
    crate::languages::vendored_dirs()
        .iter()
        .any(|d| d.eq_ignore_ascii_case(name))
}

/// Walk `root` and collect every regular file matching `predicate`.
///
/// Pruning happens **during** the walk via `ignore`'s `filter_entry`, so
/// ignoring `node_modules/` does not mean stat-ing every entry under it first
/// — the failure mode that `rglob` falls into on a real repo. Honors
/// per-directory `.gitignore`, which is the difference between a walk that
/// respects a project's policy and one that simply skips a directory named
/// `build`.
pub fn walk_targets(root: &Path, predicate: fn(&Path) -> bool) -> Vec<PathBuf> {
    let walker = WalkBuilder::new(root)
        .hidden(false)
        .git_ignore(true)
        .git_global(false)
        .git_exclude(true)
        .require_git(false)
        .sort_by_file_name(|a, b| a.cmp(b))
        .filter_entry(|entry: &DirEntry| {
            if entry.depth() == 0 {
                return true;
            }
            if entry.file_type().is_some_and(|ft| ft.is_dir())
                && is_ignored_dir(&entry.file_name().to_string_lossy())
            {
                return false;
            }
            true
        })
        .build();

    let mut found = Vec::new();
    for result in walker {
        let Ok(entry) = result else { continue };
        if entry.file_type().is_some_and(|ft| ft.is_file()) {
            let path = entry.path();
            if predicate(path) {
                found.push(path.to_path_buf());
            }
        }
    }
    found
}

/// Expand a list of explicit paths (files and/or directories) into a sorted,
/// deduplicated set of files matching `predicate`.
///
/// Dedup because `drep check a.rs .` would otherwise pay a whole LLM
/// round-trip twice. An explicit file is filtered by the same predicate as
/// directory walks, so naming `notes.txt` cannot smuggle in a type drep does
/// not read; conversely, gitignore is *not* consulted for explicit files —
/// the user naming a path is a stronger signal than the repo's ignore file.
/// Paths that do not exist are skipped silently.
pub fn expand_paths(paths: &[PathBuf], predicate: fn(&Path) -> bool) -> Vec<PathBuf> {
    let mut found = BTreeSet::new();
    for path in paths {
        if path.is_dir() {
            for file in walk_targets(path, predicate) {
                found.insert(file);
            }
        } else if path.is_file() && predicate(path) {
            found.insert(path.clone());
        }
        // Non-existent paths fall through: the user typed something the
        // filesystem does not have, and the contract is to skip without
        // erroring. Distinguishing "exists but is neither file nor dir"
        // (broken symlink, /dev/null-style specials) is not worth a branch.
    }
    found.into_iter().collect()
}

/// Why an explicitly named path produced no target.
///
/// Only *named* paths are ever rejected. A directory walk that yields nothing
/// is legitimately empty: `drep check .` in a documentation repository has
/// correctly found no code. A path the user typed is the opposite case, and
/// reporting "No issues found." for it is the single failure this codebase is
/// built to prevent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rejected {
    /// Nothing exists at that path.
    Missing,
    /// Something exists there, but this command cannot analyze it: the
    /// predicate declined the file type, or the path is neither a regular file
    /// nor a directory (a fifo, a socket, `/dev/stdin`).
    Unanalyzable,
}

/// The result of resolving a command's path arguments.
pub struct Expansion {
    /// Files to analyze, sorted and deduplicated.
    pub targets: Vec<PathBuf>,
    /// Named paths that produced nothing, and why.
    pub rejected: BTreeMap<PathBuf, Rejected>,
}

/// Resolve a command's path arguments against `root`.
///
/// The single answer to "what did the user ask for, and what could I not do
/// with it". [`expand_paths`] reports only the targets, which meant every
/// caller re-walked the same argument list with its own `exists()` /
/// `is_file()` tests to reconstruct what the expander had already decided and
/// thrown away. Two commands did that, and the reconstruction was lossy in the
/// same way in both: a named fifo satisfies neither `is_file` nor `is_dir`, so
/// it was dropped by the expander, missed by the reconstruction, and reported
/// as a clean run.
///
/// No arguments means `root`, which is how bare `drep check` means "the whole
/// tree". `root` goes through the expander like any other directory - an
/// earlier version returned it unexpanded, so `read_to_string` was handed a
/// *directory* and the plainest invocation of the primary command exited 2
/// without analyzing anything.
pub fn expand_named(paths: &[PathBuf], root: &Path, predicate: fn(&Path) -> bool) -> Expansion {
    if paths.is_empty() {
        return Expansion {
            targets: expand_paths(&[root.to_path_buf()], predicate),
            rejected: BTreeMap::new(),
        };
    }
    let mut rejected = BTreeMap::new();
    for named in paths {
        // One `metadata` call answers file/dir/missing together. The three
        // separate `exists()` / `is_dir()` / `is_file()` probes this replaced
        // were three syscalls with a window between each, and expressed the
        // "neither a regular file nor a directory" case as a negation chain
        // rather than as the arm it is.
        let verdict = match std::fs::metadata(named) {
            Err(_) => Some(Rejected::Missing),
            Ok(meta) if meta.is_dir() => None,
            Ok(meta) if meta.is_file() && predicate(named) => None,
            Ok(_) => Some(Rejected::Unanalyzable),
        };
        if let Some(verdict) = verdict {
            rejected.insert(named.clone(), verdict);
        }
    }
    Expansion {
        targets: expand_paths(paths, predicate),
        rejected,
    }
}

/// The subcommand that does analyze `path`, if any.
///
/// One table, consulted from both directions. Each command previously
/// hardcoded a pointer at the other - `check` asked `is_markdown`, `lint-docs`
/// asked `languages::detect` - which is two definitions of one question and an
/// edit in every existing command each time a file class is added.
pub fn owning_command(path: &Path) -> Option<&'static str> {
    if is_scan_target(path) {
        Some("check")
    } else if is_markdown(path) {
        Some("lint-docs")
    } else {
        None
    }
}

/// What to tell a user who named `path` at the wrong command.
///
/// `None` when no command claims the type, because drep genuinely has nothing
/// to say about a `.png` and inventing a suggestion would be worse than
/// admitting that.
pub fn redirect_hint(path: &Path) -> Option<String> {
    owning_command(path).map(|command| format!("run `drep {command}` instead"))
}

#[cfg(test)]
mod tests;
