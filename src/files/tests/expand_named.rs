//! `expand_named`, `owning_command`, `redirect_hint`.
//!
//! The rejection half is the load-bearing one: it is what stops a path the
//! user typed being dropped in silence and the run reported as clean. Both
//! commands consume it, so a gap here is a gap in both.

use std::path::Path;

use crate::files::{
    Rejected, expand_named, is_markdown, is_scan_target, owning_command, redirect_hint,
};

/// Build a tree under `root`. Each entry is `(relative path, contents)`.
fn build(root: &Path, entries: &[(&str, &str)]) {
    for (rel, body) in entries {
        let path = root.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("mkdir");
        }
        std::fs::write(&path, body).expect("write");
    }
}

#[test]
fn no_arguments_walks_the_root_and_rejects_nothing() {
    let temp = tempfile::TempDir::new().expect("tempdir");
    build(temp.path(), &[("a.md", ""), ("b.rs", "")]);

    let found = expand_named(&[], temp.path(), is_markdown);
    assert_eq!(found.targets, vec![temp.path().join("a.md")]);
    // The `.rs` file was walked past. A walk has no opinion about a type it
    // does not own, which is what makes bare `drep lint-docs` usable in a
    // mixed repository.
    assert!(found.rejected.is_empty(), "{:?}", found.rejected);
}

#[test]
fn a_named_directory_is_a_walk_not_a_rejection() {
    let temp = tempfile::TempDir::new().expect("tempdir");
    build(temp.path(), &[("src/main.rs", "")]);

    let found = expand_named(&[temp.path().join("src")], temp.path(), is_markdown);
    assert!(found.targets.is_empty());
    assert!(found.rejected.is_empty(), "{:?}", found.rejected);
}

#[test]
fn a_named_file_the_predicate_declines_is_rejected() {
    let temp = tempfile::TempDir::new().expect("tempdir");
    build(temp.path(), &[("main.rs", "")]);
    let named = temp.path().join("main.rs");

    let found = expand_named(std::slice::from_ref(&named), temp.path(), is_markdown);
    assert!(found.targets.is_empty());
    assert_eq!(found.rejected.get(&named), Some(&Rejected::Unanalyzable));
}

#[test]
fn a_named_path_that_does_not_exist_is_rejected_as_missing() {
    let temp = tempfile::TempDir::new().expect("tempdir");
    let named = temp.path().join("nope.md");

    let found = expand_named(std::slice::from_ref(&named), temp.path(), is_markdown);
    assert_eq!(found.rejected.get(&named), Some(&Rejected::Missing));
}

#[test]
fn a_named_file_the_predicate_accepts_is_a_target_and_not_rejected() {
    // The other side of every assertion above. A predicate that rejected
    // everything would satisfy them all.
    let temp = tempfile::TempDir::new().expect("tempdir");
    build(temp.path(), &[("README.md", "")]);
    let named = temp.path().join("README.md");

    let found = expand_named(std::slice::from_ref(&named), temp.path(), is_markdown);
    assert_eq!(found.targets, vec![named]);
    assert!(found.rejected.is_empty(), "{:?}", found.rejected);
}

#[cfg(unix)]
#[test]
fn a_named_path_that_is_neither_a_file_nor_a_directory_is_rejected() {
    // The case the per-command `exists()` / `is_file()` reconstruction missed:
    // a fifo exists, so it was not "missing", and it is not a regular file, so
    // it was not "the wrong type" either. It fell through both branches, was
    // dropped by the expander, and the run exited 0 clean - the banned move,
    // still open after the change that was written to close it.
    use std::os::unix::fs::FileTypeExt;

    let temp = tempfile::TempDir::new().expect("tempdir");
    let fifo = temp.path().join("pipe.md");
    let status = std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .expect("mkfifo runs");
    assert!(status.success(), "mkfifo failed");
    assert!(
        std::fs::metadata(&fifo)
            .expect("stat")
            .file_type()
            .is_fifo()
    );

    let found = expand_named(std::slice::from_ref(&fifo), temp.path(), is_markdown);
    assert!(found.targets.is_empty(), "{:?}", found.targets);
    assert_eq!(
        found.rejected.get(&fifo),
        Some(&Rejected::Unanalyzable),
        "a named path drep cannot read must never be silently dropped"
    );
}

#[test]
fn every_named_path_is_judged_not_just_the_first() {
    let temp = tempfile::TempDir::new().expect("tempdir");
    build(temp.path(), &[("a.md", ""), ("b.rs", "")]);
    let good = temp.path().join("a.md");
    let bad = temp.path().join("b.rs");
    let gone = temp.path().join("c.md");

    let found = expand_named(
        &[good.clone(), bad.clone(), gone.clone()],
        temp.path(),
        is_markdown,
    );
    assert_eq!(found.targets, vec![good]);
    assert_eq!(found.rejected.get(&bad), Some(&Rejected::Unanalyzable));
    assert_eq!(found.rejected.get(&gone), Some(&Rejected::Missing));
}

#[test]
fn owning_command_maps_each_file_class_to_one_command() {
    assert_eq!(owning_command(Path::new("main.rs")), Some("check"));
    assert_eq!(owning_command(Path::new("app.py")), Some("check"));
    assert_eq!(owning_command(Path::new("README.md")), Some("lint-docs"));
    // No command claims these, and drep must not invent one.
    assert_eq!(owning_command(Path::new("logo.png")), None);
    assert_eq!(owning_command(Path::new("Makefile")), None);
}

#[test]
fn owning_command_agrees_with_the_predicates_it_is_derived_from() {
    // The point of having one table: `check`'s predicate and `lint-docs`'
    // predicate cannot disagree with the answer users are given about which
    // command to run.
    for name in ["main.rs", "app.py", "README.md", "logo.png", "Makefile"] {
        let path = Path::new(name);
        match owning_command(path) {
            Some("check") => assert!(is_scan_target(path), "{name}"),
            Some("lint-docs") => assert!(is_markdown(path), "{name}"),
            Some(other) => panic!("unknown command {other} for {name}"),
            None => assert!(!is_scan_target(path) && !is_markdown(path), "{name}"),
        }
    }
}

#[test]
fn redirect_hint_names_the_command_or_says_nothing() {
    assert_eq!(
        redirect_hint(Path::new("README.md")).as_deref(),
        Some("run `drep lint-docs` instead")
    );
    assert_eq!(
        redirect_hint(Path::new("main.rs")).as_deref(),
        Some("run `drep check` instead")
    );
    assert_eq!(redirect_hint(Path::new("logo.png")), None);
}
