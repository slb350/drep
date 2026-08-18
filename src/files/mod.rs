//! The only file-target policy.
//!
//! Every code path in drep that decides which files to look at goes through
//! these predicates, so a Python file is a file is a file regardless of whether
//! it was discovered by a full scan, the staged-files index, or a
//! `--diff <ref>` query. Markdown is included because the documentation
//! analyzer needs it; it is not a registered language, so it never enters the
//! language table.
//!
//! Walking is the `ignore` crate rather than `std::fs::read_dir` so a project's
//! gitignore is honoured without a second pass, and vendored directories are
//! pruned during the walk (rather than collected-then-filtered, which would
//! `stat` every entry under `node_modules/` before discarding them).
//!
//! Discovery of explicit filenames (`drep check a.rs .`) deliberately ignores
//! gitignore: the user naming a path is a stronger signal than a repo-wide
//! pattern. `expand_paths` enforces that; `walk_targets` enforces the inverse.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use ignore::{DirEntry, WalkBuilder};

use crate::languages;

/// True iff `path`'s lowercased extension matches `target_without_dot`.
///
/// `target` is e.g. `"py"`; `"FOO.PY"` matches, `"py"` (no extension) does not.
fn extension_is(path: &Path, target: &str) -> bool {
    match path.extension() {
        None => false,
        Some(ext) => ext.eq_ignore_ascii_case(target),
    }
}

/// True iff `path`'s lowercased extension appears in `targets_with_dots`.
///
/// Used by the walkers; the inline `extension_is` is for single-suffix
/// predicates where allocation-free comparison is clearer.
fn extension_in(path: &Path, targets_with_dots: &[&str]) -> bool {
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        return false;
    };
    let lower = ext.to_ascii_lowercase();
    let dotted = format!(".{lower}");
    targets_with_dots.iter().any(|t| **t == dotted)
}

/// Any registered language's source file, plus markdown for the documentation
/// analyzer. Markdown is *not* a registered language: adding it to the registry
/// would mean an LLM "language check" against prose, which is not what the
/// doc-specialist does.
pub fn is_scan_target(path: &Path) -> bool {
    extension_in(path, languages::source_extensions()) || is_markdown(path)
}

/// Python source file. Kept distinct from `is_scan_target` because the
/// docstring pass runs `ast.parse`, and adding a language must never widen
/// that filter.
pub fn is_python_source(path: &Path) -> bool {
    extension_is(path, "py")
}

/// Markdown document.
pub fn is_markdown(path: &Path) -> bool {
    extension_is(path, "md")
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

#[cfg(test)]
mod tests;
