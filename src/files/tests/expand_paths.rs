//! `expand_paths`.
//!
//! The dedup contract is the load-bearing one: `drep check a.rs .` *must* not
//! pay the LLM twice for the same file. The gitignore-asymmetry is also
//! deliberate — the user naming a path is the strongest signal we have.

use std::fs;
use std::path::{Path, PathBuf};

use crate::files::expand_paths;

/// Build a tree under `root` matching `entries`. See `walk_targets::build`
/// for the same helper; duplicated because each module owns its fixtures.
fn build(root: &Path, entries: &[(&str, &str)]) {
    for (rel, body) in entries {
        let path = root.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parents");
        }
        fs::write(&path, body).expect("write file");
    }
}

fn is_scan_targetish(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("rs" | "py" | "md")
    )
}

#[test]
fn duplicate_paths_collapse_to_a_single_file() {
    // `a.rs` is named explicitly AND inside `.`. The output must contain it
    // exactly once — paying for it twice means a full LLM round-trip per
    // duplicate, not a duplicate report line.
    let temp = tempfile::TempDir::new().expect("tempdir");
    let root = temp.path();
    build(root, &[("a.rs", "")]);

    let inputs = vec![root.join("a.rs"), root.to_path_buf()];
    let result = expand_paths(&inputs, is_scan_targetish);
    let result_strs: Vec<String> = result
        .iter()
        .map(|p| {
            p.strip_prefix(root)
                .unwrap_or(p)
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert_eq!(
        result_strs,
        vec!["a.rs".to_owned()],
        "duplicates must collapse: {result_strs:?}"
    );
}

#[test]
fn output_is_sorted_and_stable_across_calls() {
    // Sorted by `BTreeSet` — assert the property rather than the order so
    // adding files doesn't lock the test to a fragile sequence.
    let temp = tempfile::TempDir::new().expect("tempdir");
    let root = temp.path();
    build(root, &[("z.rs", ""), ("a.rs", ""), ("m.rs", "")]);

    let once = expand_paths(&[root.to_path_buf()], is_scan_targetish);
    let twice = expand_paths(&[root.to_path_buf()], is_scan_targetish);

    assert_eq!(once, twice, "two calls must return identical vectors");
    let mut sorted = once.clone();
    sorted.sort();
    assert_eq!(
        once, sorted,
        "output should already be sorted, not depend on the walker"
    );
}

#[test]
fn explicit_filenames_are_honoured_even_when_gitignored() {
    // The deliberate asymmetry with `walk_targets`: a `.gitignore` matching a
    // file does NOT remove it from an explicit invocation. The user named it;
    // the user gets to analyze it.
    let temp = tempfile::TempDir::new().expect("tempdir");
    let root = temp.path();
    build(root, &[(".gitignore", "skipped.rs\n"), ("skipped.rs", "")]);

    let result = expand_paths(&[root.join("skipped.rs")], is_scan_targetish);
    assert_eq!(
        result,
        vec![root.join("skipped.rs")],
        "explicit paths skip gitignore: {result:?}"
    );
}

#[test]
fn explicit_filenames_failing_predicate_are_dropped() {
    // Naming a file with the wrong type cannot smuggle it into analysis.
    let temp = tempfile::TempDir::new().expect("tempdir");
    let root = temp.path();
    build(root, &[("notes.txt", ""), ("keep.rs", "")]);

    let result = expand_paths(
        &[root.join("notes.txt"), root.join("keep.rs")],
        is_scan_targetish,
    );
    let names: Vec<String> = result
        .iter()
        .map(|p| {
            p.strip_prefix(root)
                .unwrap_or(p)
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert_eq!(names, vec!["keep.rs".to_owned()]);
}

#[test]
fn paths_that_do_not_exist_are_silently_skipped() {
    let temp = tempfile::TempDir::new().expect("tempdir");
    let root = temp.path();
    build(root, &[("real.rs", "")]);

    let missing: PathBuf = root.join("does_not_exist.rs");

    let inputs = vec![root.join("real.rs"), missing, root.to_path_buf()];
    let result = expand_paths(&inputs, is_scan_targetish);
    let names: Vec<String> = result
        .iter()
        .map(|p| {
            p.strip_prefix(root)
                .unwrap_or(p)
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert_eq!(names, vec!["real.rs".to_owned()]);
}
