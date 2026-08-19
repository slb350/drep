//! `staged_hunks` and `hunks_since`: criteria 14-18.

use std::fs;

use crate::diff::hunks::HunkLine;
use crate::diff::{hunks_since, staged_hunks};
use crate::files;

use super::support::{GitRepo, run_in};

#[tokio::test]
async fn staged_hunks_reports_a_real_line_number_for_a_modified_staged_file() {
    let repo = GitRepo::init().await;
    let root = repo.root();
    let path = root.join("feature.rs");

    fs::write(&path, "line 1\nline 2\nline 3\nline 4\nline 5\n").expect("write");
    run_in(root, &["add", "feature.rs"]).await;
    repo.commit_all("initial").await;

    fs::write(&path, "line 1\nline 2\nCHANGED\nline 4\nline 5\n").expect("write");
    run_in(root, &["add", "feature.rs"]).await;

    let hunks = staged_hunks(root, files::is_scan_target)
        .await
        .expect("staged_hunks");
    assert_eq!(hunks.len(), 1, "expected one hunk, got {hunks:?}");
    let h = &hunks[0];
    let line_3 = h
        .numbered_new_lines()
        .find(|(n, content)| *n == 3 && content.contains("CHANGED"))
        .expect("line 3 should appear with its real file line number");
    assert!(
        line_3.1.contains("CHANGED"),
        "the hunk should contain the new line text, got {h:?}"
    );
}

#[tokio::test]
async fn staged_hunks_works_on_a_repo_with_no_commits_yet() {
    // The empty-tree fallback: a fresh `git init` has no HEAD. Without
    // the fallback, this would error and the pre-commit install on a
    // brand new repo would block the user's first commit.
    let repo = GitRepo::init_no_commits().await;
    let root = repo.root();

    fs::write(root.join("first.rs"), "alpha\nbeta\ngamma\n").expect("write");
    run_in(root, &["add", "first.rs"]).await;

    let hunks = staged_hunks(root, files::is_scan_target)
        .await
        .expect("staged_hunks no-commits");
    assert_eq!(hunks.len(), 1, "expected one hunk, got {hunks:?}");
    assert_eq!(hunks[0].file_path.to_string_lossy(), "first.rs");
}

#[tokio::test]
async fn staged_hunks_returns_no_hunk_for_cargo_lock() {
    let repo = GitRepo::init().await;
    let root = repo.root();

    fs::write(root.join("Cargo.lock"), "before\n").expect("write");
    run_in(root, &["add", "Cargo.lock"]).await;
    repo.commit_all("seed Cargo.lock").await;
    fs::write(root.join("Cargo.lock"), "after\n").expect("write");
    run_in(root, &["add", "Cargo.lock"]).await;

    let hunks = staged_hunks(root, files::is_scan_target)
        .await
        .expect("staged_hunks");
    assert!(
        hunks.is_empty(),
        "Cargo.lock must be dropped by is_scan_target, got {hunks:?}"
    );
}

#[tokio::test]
async fn hunks_since_uses_three_dot_semantics_not_two_dot() {
    // The discriminating case. Same fork shape as the existing
    // `three_dot_excludes_base_modifications_that_two_dot_would_report`
    // test, but here we exercise the hunk form rather than the file-name
    // form. The base branch *modifies* a shared file, which a two-dot
    // diff reports as `M`, which `--diff-filter=ACMR` keeps. A two-dot
    // implementation would therefore return a hunk for `shared.rs` even
    // though this branch never touched it. We assert that no such hunk
    // appears.
    let repo = GitRepo::init().await;
    let root = repo.root();

    fs::write(root.join("shared.rs"), "v1\n").expect("write");
    run_in(root, &["add", "."]).await;
    repo.commit_all("seed").await;
    repo.create_branch("feature").await;

    repo.checkout("feature").await;
    fs::write(root.join("on_branch.rs"), "branch\n").expect("write");
    run_in(root, &["add", "on_branch.rs"]).await;
    repo.commit_all("feature change").await;

    repo.checkout("main").await;
    fs::write(root.join("shared.rs"), "v2\n").expect("write");
    run_in(root, &["add", "shared.rs"]).await;
    repo.commit_all("base modifies shared.rs after the fork")
        .await;

    repo.checkout("feature").await;

    let hunks = hunks_since(root, "main", files::is_scan_target)
        .await
        .expect("hunks_since");
    let names: Vec<String> = hunks
        .iter()
        .map(|h| h.file_path.to_string_lossy().into_owned())
        .collect();

    assert!(
        names.iter().any(|n| n.ends_with("on_branch.rs")),
        "the branch's own change must be reported, got {names:?}"
    );
    assert!(
        !names.iter().any(|n| n.ends_with("shared.rs")),
        "shared.rs was modified on the base, not by this branch - a two-dot \
         diff reports it as M and ACMR keeps it. got {names:?}"
    );
}

#[tokio::test]
async fn staged_hunks_requests_at_least_fifteen_context_lines() {
    // 40-line file, change at line 20. With --unified=20 the diff
    // reaches back to line 1; if `--unified` is missing (default 3)
    // the context before the change is far smaller. Counting Context
    // lines whose new-line number is below 20 hits at least 15 when
    // the context budget actually reached git.
    let repo = GitRepo::init().await;
    let root = repo.root();

    let mut body = String::new();
    for i in 1..=40 {
        body.push_str(&format!("line {i}\n"));
    }
    let path = root.join("wide.rs");
    fs::write(&path, &body).expect("write");
    run_in(root, &["add", "wide.rs"]).await;
    repo.commit_all("seed").await;

    let mutated = body.replacen("line 20\n", "CHANGED line 20\n", 1);
    fs::write(&path, &mutated).expect("write");
    run_in(root, &["add", "wide.rs"]).await;

    let hunks = staged_hunks(root, files::is_scan_target)
        .await
        .expect("staged_hunks");
    assert_eq!(hunks.len(), 1);
    let h = &hunks[0];
    let before_change: usize = h
        .lines
        .iter()
        .take_while(|l| match l {
            HunkLine::Context(s) => !s.contains("CHANGED"),
            _ => true,
        })
        .filter(|l| matches!(l, HunkLine::Context(_)))
        .count();
    assert!(
        before_change >= 15,
        "expected ≥15 context lines before the change (proving --unified=20), got {before_change}"
    );
}

/// `--diff` in a repo with no commits reports that plainly.
///
/// A three-dot spec is a symmetric difference between two *commits*; the empty
/// tree is not one, so the fallback that used to live here could never produce
/// a diff - it only turned "no commits yet" into git's opaque "Invalid
/// symmetric difference expression". The message now names the situation and
/// the way out.
///
/// The `--tip` half pins that an explicit tip is used verbatim rather than
/// being replaced by any fallback: without it, ignoring `tip` would pass.
#[tokio::test]
async fn hunks_between_reports_plainly_when_the_repo_has_no_commits() {
    let repo = GitRepo::init_no_commits().await;
    let root = repo.root();

    let err = crate::diff::hunks_between(root, EMPTY_TREE_SHA, None, files::is_scan_target)
        .await
        .expect_err("there is no HEAD to diff to");
    let rendered = format!("{err}");
    assert!(
        rendered.contains("no commits yet"),
        "the message must name the situation, got: {rendered}"
    );
    assert!(
        !rendered.contains("symmetric difference"),
        "and must not leak git's version of it, got: {rendered}"
    );

    // An explicit tip skips the HEAD probe entirely, so this fails inside git
    // rather than at the guard above.
    let err = crate::diff::hunks_between(root, EMPTY_TREE_SHA, Some("HEAD"), files::is_scan_target)
        .await
        .expect_err("HEAD is unborn");
    assert!(
        !format!("{err}").contains("no commits yet"),
        "an explicit tip is used verbatim, so the guard must not fire"
    );
}

/// The empty tree's well-known SHA, as git itself defines it.
const EMPTY_TREE_SHA: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";
