//! `changed_since`: criteria 27-28.
//!
//! Two tests build a real divergence between branches and assert that `...`
//! reports only the side that branched off, never what landed on the *base*
//! after the fork.
//!
//! The distinction between them is the whole point, and it is not obvious.
//! `three_dot_does_not_include_base_changes_added_after_the_fork` uses an
//! **added** file on the base, which a two-dot diff reports as a *deletion*
//! (present in the base, absent here) - and `--diff-filter=ACMR` drops
//! deletions, so that test passes under a two-dot implementation too. It
//! documents intent but proves nothing.
//!
//! `three_dot_excludes_base_modifications_that_two_dot_would_report` is the
//! one that actually discriminates: the base *modifies* a shared file, which a
//! two-dot diff reports as `M`, which the filter keeps. Verified by flipping
//! `mod.rs` to `..` and confirming only the second test fails.

use std::fs;

use crate::diff::changed_since;

use super::support::{GitRepo, run_in};

#[tokio::test]
async fn three_dot_does_not_include_base_changes_added_after_the_fork() {
    let repo = GitRepo::init().await;
    let root = repo.root();

    // Topology: a seed commit, then branch `feature`. `feature` adds
    // `on_branch.rs`; meanwhile `main` advances with `new_on_main.rs` AFTER
    // the fork. We stand on `feature` (the model for a pre-push hook) and
    // ask `changed_since("main")`. That diff is `main...feature`, which
    // shows only what `feature` changed since the merge base — never what
    // `main` accumulated afterwards.
    fs::write(root.join("seed.rs"), "common\n").expect("write");
    run_in(root, &["add", "."]).await;
    repo.commit_all("seed").await;
    repo.create_branch("feature").await;

    repo.checkout("feature").await;
    fs::write(root.join("on_branch.rs"), "").expect("write");
    run_in(root, &["add", "on_branch.rs"]).await;
    repo.commit_all("feature change").await;

    repo.checkout("main").await;
    fs::write(root.join("new_on_main.rs"), "").expect("write");
    run_in(root, &["add", "new_on_main.rs"]).await;
    repo.commit_all("main change after fork").await;

    // Stand on feature again: that is the user invoking `drep check --diff
    // main` from a feature branch before pushing.
    repo.checkout("feature").await;

    let changed = changed_since(root, "main").await.expect("changed_since");
    let names: Vec<String> = changed
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();

    assert!(
        names.iter().any(|n| n == "on_branch.rs"),
        "the feature-branch change should appear: {names:?}"
    );
    assert!(
        !names.iter().any(|n| n == "new_on_main.rs"),
        "a file added to main AFTER the fork must NOT appear in feature's diff: {names:?}"
    );
}

#[tokio::test]
async fn returns_an_error_when_the_ref_does_not_exist() {
    let repo = GitRepo::init().await;
    let root = repo.root();

    let err = changed_since(root, "does/not/exist")
        .await
        .expect_err("non-existent ref must error");
    let display = err.to_string();
    assert!(
        display.contains("git") || display.contains("exit"),
        "expected a git error, got {display:?}"
    );
}

#[tokio::test]
async fn three_dot_excludes_base_modifications_that_two_dot_would_report() {
    // The discriminating case. Same fork shape as above, but the base branch
    // *modifies* a file that already exists on both sides rather than adding a
    // new one. Under `main..feature` that file appears as `M` - feature "reverts"
    // main's edit relative to main's tip - and `--diff-filter=ACMR` keeps
    // modifications, so a two-dot implementation reports `shared.rs` as changed
    // by this branch when the branch never touched it.
    let repo = GitRepo::init().await;
    let root = repo.root();

    fs::write(root.join("shared.rs"), "v1\n").expect("write");
    run_in(root, &["add", "."]).await;
    repo.commit_all("seed").await;
    repo.create_branch("feature").await;

    repo.checkout("feature").await;
    fs::write(root.join("on_branch.rs"), "").expect("write");
    run_in(root, &["add", "on_branch.rs"]).await;
    repo.commit_all("feature change").await;

    repo.checkout("main").await;
    fs::write(root.join("shared.rs"), "v2\n").expect("write");
    run_in(root, &["add", "shared.rs"]).await;
    repo.commit_all("base modifies a shared file after the fork")
        .await;

    repo.checkout("feature").await;

    let changed = changed_since(root, "main").await.expect("changed_since");
    let names: Vec<String> = changed
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();

    assert!(
        names.iter().any(|n| n.ends_with("on_branch.rs")),
        "the branch's own change must be reported, got {names:?}"
    );
    assert!(
        !names.iter().any(|n| n.ends_with("shared.rs")),
        "shared.rs was modified on the base, not by this branch - a two-dot \
         diff reports it as M and the ACMR filter keeps it. got {names:?}"
    );
}
