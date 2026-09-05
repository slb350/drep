//! Branch-diff scope and invalid-ref errors.

use std::fs;
use std::path::PathBuf;

use crate::diff::changed_since;

use super::support::GitRepo;

#[tokio::test]
async fn three_dot_excludes_base_additions_and_modifications() {
    let repo = GitRepo::init().await;
    let root = repo.root();

    fs::write(root.join("shared.rs"), "v1\n").expect("write");
    repo.commit_all("seed").await;
    repo.create_branch("feature").await;

    repo.checkout("feature").await;
    fs::write(root.join("on_branch.rs"), "").expect("write");
    repo.commit_all("feature change").await;

    repo.checkout("main").await;
    fs::write(root.join("new_on_main.rs"), "").expect("write");
    // A base addition alone cannot distinguish two-dot from three-dot:
    // two-dot reports it as a deletion, which ACMR filters out. A modified
    // shared file survives that filter and exposes the wrong diff scope.
    fs::write(root.join("shared.rs"), "v2\n").expect("write");
    repo.commit_all("main change after fork").await;

    repo.checkout("feature").await;

    let changed = changed_since(root, "main").await.expect("changed_since");
    assert_eq!(
        changed,
        vec![PathBuf::from("on_branch.rs")],
        "only the feature branch's changes belong in its diff"
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
