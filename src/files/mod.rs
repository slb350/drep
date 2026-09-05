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
/// Explicit files bypass gitignore. Missing and unsupported paths are skipped;
/// callers needing those rejections use [`expand_named`]. Empty input is empty.
pub fn expand_paths(paths: &[PathBuf], predicate: fn(&Path) -> bool) -> Vec<PathBuf> {
    expand(paths, predicate).targets
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
/// Classify each explicit path once, collecting both targets and rejections.
/// No arguments expands `root` without treating it as an explicitly named path.
pub fn expand_named(paths: &[PathBuf], root: &Path, predicate: fn(&Path) -> bool) -> Expansion {
    if paths.is_empty() {
        return Expansion {
            targets: expand_paths(&[root.to_path_buf()], predicate),
            rejected: BTreeMap::new(),
        };
    }
    expand(paths, predicate)
}

fn expand(paths: &[PathBuf], predicate: fn(&Path) -> bool) -> Expansion {
    let mut targets = BTreeSet::new();
    let mut rejected = BTreeMap::new();
    for named in paths {
        let verdict = match std::fs::metadata(named) {
            Err(_) => Some(Rejected::Missing),
            Ok(meta) if meta.is_dir() => {
                targets.extend(walk_targets(named, predicate));
                None
            }
            Ok(meta) if meta.is_file() && predicate(named) => {
                targets.insert(named.clone());
                None
            }
            Ok(_) => Some(Rejected::Unanalyzable),
        };
        if let Some(verdict) = verdict {
            rejected.insert(named.clone(), verdict);
        }
    }
    Expansion {
        targets: targets.into_iter().collect(),
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
