//! `diverging_from`: what a linter reading the working tree would see that a
//! branch range's tip does not hold.

use std::fs;
use std::path::PathBuf;

use crate::diff::{GitError, diverging_from};

use super::support::GitRepo;

/// A tip that is not checked out differs wherever the index differs from it,
/// in both directions: a file the tip changed, one only the tip holds, and one
/// only the checkout holds. A working-tree edit differs as staged mode says,
/// and a path in both sets is named once, the list sorted.
#[tokio::test]
async fn lists_the_index_against_the_tip_and_the_working_tree_against_the_index() {
    let repo = GitRepo::init().await;
    let root = repo.root();
    fs::write(root.join("changed.py"), "a = 1\n").expect("write changed");
    fs::write(root.join("only_base.py"), "b = 1\n").expect("write only_base");
    fs::write(root.join("same.py"), "c = 1\n").expect("write same");
    repo.commit_all("base").await;
    repo.create_branch("feature").await;
    repo.checkout("feature").await;
    fs::write(root.join("changed.py"), "a = 2\n").expect("tip edit");
    fs::write(root.join("only_tip.py"), "d = 1\n").expect("tip file");
    fs::remove_file(root.join("only_base.py")).expect("tip deletion");
    repo.commit_all("feature").await;
    repo.checkout("main").await;
    // The working tree's paths are gathered ahead of the index's, so `zz.py`
    // is what pins the sort and `changed.py`, in both sets, the dedup.
    fs::write(root.join("zz.py"), "e = 1\n").expect("untracked");
    fs::write(root.join("changed.py"), "a = 3\n").expect("edit in both sets");

    let diverging = diverging_from(root, "feature")
        .await
        .expect("diverging_from");
    assert_eq!(
        diverging.differing,
        vec![
            PathBuf::from("changed.py"),
            PathBuf::from("only_base.py"),
            PathBuf::from("only_tip.py"),
            PathBuf::from("zz.py"),
        ],
        "every path whose working-tree state the tip does not hold, once each"
    );
    assert!(diverging.index_only.is_empty());
}

/// A clean checkout of the tip holds it exactly.
#[tokio::test]
async fn a_clean_checkout_of_the_tip_differs_nowhere() {
    let repo = GitRepo::init().await;
    let root = repo.root();
    fs::write(root.join("a.py"), "a = 1\n").expect("write");
    repo.commit_all("base").await;

    let diverging = diverging_from(root, "HEAD").await.expect("diverging_from");
    assert!(diverging.differing.is_empty(), "{:?}", diverging.differing);
}

/// Run from a subdirectory, a file above it that the tip changed is named
/// from there.
#[tokio::test]
async fn names_a_tip_difference_from_the_subdirectory() {
    let repo = GitRepo::init().await;
    let root = repo.root();
    fs::create_dir_all(root.join("sub")).expect("subdirectory");
    fs::write(root.join("sub/keep.py"), "k = 1\n").expect("write keep");
    fs::write(root.join("top.py"), "a = 1\n").expect("write top");
    repo.commit_all("base").await;
    repo.create_branch("feature").await;
    repo.checkout("feature").await;
    fs::write(root.join("top.py"), "a = 2\n").expect("tip edit");
    repo.commit_all("feature").await;
    repo.checkout("main").await;

    let diverging = diverging_from(&root.join("sub"), "feature")
        .await
        .expect("diverging_from");
    assert_eq!(diverging.differing, vec![PathBuf::from("../top.py")]);
}

/// A tip that begins with `-` is refused before git sees it: `git diff`
/// reads `--output=<file>` in that position as an option and writes the file.
#[tokio::test]
async fn refuses_a_tip_that_looks_like_a_flag_without_running_git() {
    let repo = GitRepo::init().await;
    let root = repo.root();
    let written = root.join("written");
    let tip = format!("--output={}", written.display());

    let result = diverging_from(root, &tip).await;
    assert!(
        matches!(&result, Err(GitError::NonZero { code: None, stderr }) if stderr.contains("looks like a flag")),
        "got {result:?}"
    );
    assert!(
        !written.exists(),
        "git must never have been handed the flag"
    );
}
