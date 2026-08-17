//! `walk_targets`.
//!
//! The whole point of this function is to not stat every entry under
//! `node_modules/` or `venv/`. The tests build a temp tree with an ignored
//! directory full of scan-target files and assert none of them leak back.

use std::fs;
use std::path::Path;

use crate::files::walk_targets;

/// Build a tree under `root` with the entries described by `entries`.
///
/// Each entry is `(relative_path, contents)`. Missing parent directories are
/// created with `fs::create_dir_all`. Tests use this rather than `tempdir`
/// plus ad-hoc writes so the setup is local to one function and one path
/// spelling can be re-read.
fn build(root: &Path, entries: &[(&str, &str)]) {
    for (rel, body) in entries {
        let path = root.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parents");
        }
        fs::write(&path, body).expect("write file");
    }
}

/// Normalise for assertion: strip the `root` prefix so paths look like what a
/// human wrote in setup, not absolute paths under `/var/folders/.../T/`.
fn relative(root: &Path, paths: Vec<std::path::PathBuf>) -> Vec<String> {
    paths
        .into_iter()
        .map(|p| {
            p.strip_prefix(root)
                .unwrap_or(&p)
                .to_string_lossy()
                .into_owned()
        })
        .collect()
}

#[test]
fn finds_files_nested_multiple_levels_deep() {
    let temp = tempfile::TempDir::new().expect("tempdir");
    let root = temp.path();
    build(
        root,
        &[
            ("top.rs", ""),
            ("a/one.rs", ""),
            ("a/b/two.rs", ""),
            ("a/b/c/three.rs", ""),
        ],
    );

    let found = relative(root, walk_targets(root, is_rs_file));
    assert!(found.contains(&"top.rs".to_owned()), "{found:?}");
    assert!(found.contains(&"a/b/c/three.rs".to_owned()), "{found:?}");
    assert_eq!(found.len(), 4);
}

fn is_rs_file(path: &Path) -> bool {
    path.extension().is_some_and(|e| e == "rs")
}

#[test]
fn does_not_descend_into_an_ignored_directory() {
    // The whole reason for a custom walker: every scanner in drep would have
    // paid a gitignored-tree tax before this existed. The ignored directory
    // here is full of files that *would* match the predicate if seen.
    let temp = tempfile::TempDir::new().expect("tempdir");
    let root = temp.path();
    build(
        root,
        &[
            ("kept.rs", ""),
            ("node_modules/should_skip.rs", ""),
            ("node_modules/dep/nested.rs", ""),
            ("venv/also_skip.py", ""),
        ],
    );

    let found = relative(root, walk_targets(root, is_scan_targetish));
    assert_eq!(
        found,
        vec!["kept.rs".to_owned()],
        "ignored directory contents must not appear: {found:?}"
    );
}

#[test]
fn does_not_return_non_target_files() {
    // walk_targets is "scan-target-aware" by virtue of the predicate: a
    // `.txt` next to a `.rs` only the `.rs` should come back, and the rest
    // should be dropped without descending into them.
    let temp = tempfile::TempDir::new().expect("tempdir");
    let root = temp.path();
    build(
        root,
        &[
            ("keep.rs", ""),
            ("notes.txt", ""),
            ("Cargo.lock", ""),
            ("logo.png", ""),
            ("README.md", ""),
            ("Makefile", ""),
        ],
    );

    let found = relative(root, walk_targets(root, is_scan_targetish));
    assert_eq!(found, vec!["README.md".to_owned(), "keep.rs".to_owned()]);
}

#[test]
fn respects_a_gitignore_in_the_walked_tree() {
    // `ignore`'s purpose here: a repo's `target/` (gitignored) is not its
    // code. The crate also honours nested `.gitignore` files. We verify the
    // first contract by putting a `.gitignore` at the root.
    let temp = tempfile::TempDir::new().expect("tempdir");
    let root = temp.path();
    build(
        root,
        &[
            (".gitignore", "ignored/\n"),
            ("keep.rs", ""),
            ("ignored/skipped.rs", ""),
        ],
    );

    let found = relative(root, walk_targets(root, is_rs_file));
    assert_eq!(
        found,
        vec!["keep.rs".to_owned()],
        "gitignored entries should not be returned: {found:?}"
    );
}

fn is_scan_targetish(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("rs" | "py" | "md")
    )
}
