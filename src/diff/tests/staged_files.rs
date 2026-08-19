//! `staged_files`: criteria 22-26.

use std::fs;
use std::path::Path;

use crate::diff::staged_files;
use crate::files;

use super::support::GitRepo;

#[tokio::test]
async fn returns_a_staged_added_source_file() {
    let repo = GitRepo::init().await;
    let root = repo.root();

    fs::write(root.join("feature.rs"), "fn main() {}\n").expect("write");
    crate::diff::tests::support::run_in(root, &["add", "feature.rs"]).await;

    let files = staged_files(root, files::is_scan_target)
        .await
        .expect("staged_files");
    assert!(
        files.iter().any(|p| p == Path::new("feature.rs")),
        "feature.rs should appear in staged_files, got {files:?}"
    );
}

#[tokio::test]
async fn excludes_a_staged_deletion() {
    // `--diff-filter=ACMR` strips deletions: a deleted file cannot be
    // analyzed, and reporting it would look like an unreadable file rather
    // than an absent one. The test commits a file, then `git rm`s it, and
    // checks the result is empty (not "deletion included").
    let repo = GitRepo::init().await;
    let root = repo.root();

    fs::write(root.join("victim.rs"), "x").expect("write");
    crate::diff::tests::support::run_in(root, &["add", "victim.rs"]).await;
    repo.commit_all("add victim").await;
    // `git rm` (without `--cached`) removes the file from the working tree
    // AND stages the deletion in one shot, which is what the user types
    // before running `drep check --staged`.
    crate::diff::tests::support::run_in(root, &["rm", "-f", "victim.rs"]).await;

    let files = staged_files(root, files::is_scan_target)
        .await
        .expect("staged_files");
    assert!(
        !files.iter().any(|p| p == Path::new("victim.rs")),
        "deletions must be excluded, got {files:?}"
    );
}

#[tokio::test]
async fn excludes_a_staged_non_target_file() {
    // `notes.txt` is in the index but `is_scan_target` rejects it; staging
    // alone does not promote it to "needs analysis".
    let repo = GitRepo::init().await;
    let root = repo.root();

    fs::write(root.join("notes.txt"), "scratchpad\n").expect("write");
    crate::diff::tests::support::run_in(root, &["add", "notes.txt"]).await;

    let files = staged_files(root, files::is_scan_target)
        .await
        .expect("staged_files");
    assert!(
        !files.iter().any(|p| p == Path::new("notes.txt")),
        "non-target files must be filtered out, got {files:?}"
    );
}

#[tokio::test]
async fn works_on_a_repo_with_no_commits_yet() {
    // The empty-tree fallback: a fresh `git init` has no `HEAD`. Without the
    // fallback, this would error and the `pre-commit` install on a brand
    // new repo would block the user's very first commit.
    let repo = GitRepo::init_no_commits().await;
    let root = repo.root();

    fs::write(root.join("first.rs"), "").expect("write");
    crate::diff::tests::support::run_in(root, &["add", "first.rs"]).await;

    let files = staged_files(root, files::is_scan_target)
        .await
        .expect("staged_files no-commits");
    assert!(
        files.iter().any(|p| p == Path::new("first.rs")),
        "expected first.rs in staged_files, got {files:?}"
    );
}

#[tokio::test]
async fn errors_when_the_directory_is_not_a_git_repository() {
    // "Could not ask git" must NOT collapse into "no files changed". A user
    // who runs `drep check --staged` outside a repo needs to see the error
    // and run `git init` (or omit `--staged`), not get a silent pass.
    let dir = tempfile::tempdir().expect("tempdir");

    let err = staged_files(dir.path(), files::is_scan_target)
        .await
        .expect_err("must error");
    let display = err.to_string();
    assert!(
        display.contains("git")
            && (display.contains("not a git repository")
                || display.contains("exit")
                || display.contains("spawned")),
        "expected a git error message, got {display:?}"
    );
}

/// The file class is the caller's, not this module's.
///
/// `staged_files` hardcoded `is_scan_target`, which is registered-language
/// sources - so `lint-docs --staged` had no way to ask git for the staged
/// *markdown*. The two predicates are disjoint by construction, so one call
/// with each over the same index is the whole contract.
#[tokio::test]
async fn the_caller_chooses_the_file_class() {
    let repo = GitRepo::init().await;
    let root = repo.root();

    fs::write(root.join("feature.rs"), "fn main() {}\n").expect("write");
    fs::write(root.join("README.md"), "# Title\n").expect("write");
    crate::diff::tests::support::run_in(root, &["add", "feature.rs", "README.md"]).await;

    let sources = staged_files(root, files::is_scan_target)
        .await
        .expect("staged sources");
    assert_eq!(sources, vec![Path::new("feature.rs").to_path_buf()]);

    let markdown = staged_files(root, files::is_markdown)
        .await
        .expect("staged markdown");
    assert_eq!(markdown, vec![Path::new("README.md").to_path_buf()]);
}
