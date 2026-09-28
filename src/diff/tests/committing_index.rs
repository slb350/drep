//! Which inherited `GIT_INDEX_FILE` the staged diff reads: the index a hook's commit is made from, when the hook runs in the repository drep was asked about.

use std::ffi::OsStr;

use crate::diff::git::owned_index;

#[test]
fn an_index_of_the_hooks_own_repository_is_kept() {
    let git_dir = tempfile::tempdir().expect("tempdir");
    let index = git_dir.path().join("index.lock");
    assert_eq!(
        owned_index(index.as_os_str(), git_dir.path(), git_dir.path()).expect("resolves"),
        Some(index)
    );
}

#[test]
fn an_index_the_committer_placed_outside_the_git_directory_is_kept() {
    let git_dir = tempfile::tempdir().expect("tempdir");
    let elsewhere = tempfile::tempdir().expect("tempdir");
    let index = elsewhere.path().join("alternate.index");
    assert_eq!(
        owned_index(index.as_os_str(), git_dir.path(), git_dir.path()).expect("resolves"),
        Some(index)
    );
}

#[test]
fn a_hook_in_another_repository_names_no_index_for_this_one() {
    let ours = tempfile::tempdir().expect("tempdir");
    let theirs = tempfile::tempdir().expect("tempdir");
    let index = theirs.path().join("index");
    assert_eq!(
        owned_index(index.as_os_str(), theirs.path(), ours.path()).expect("resolves"),
        None
    );
}

#[cfg(unix)]
#[test]
fn a_symlinked_spelling_of_the_git_directory_is_the_same_repository() {
    let git_dir = tempfile::tempdir().expect("tempdir");
    let alias = tempfile::tempdir().expect("tempdir");
    let linked = alias.path().join("linked");
    std::os::unix::fs::symlink(git_dir.path(), &linked).expect("link the git directory");
    let index = git_dir.path().join("index.lock");
    assert_eq!(
        owned_index(index.as_os_str(), &linked, git_dir.path()).expect("resolves"),
        Some(index)
    );
}

#[test]
fn a_relative_index_resolves_against_the_hooks_working_directory() {
    let git_dir = tempfile::tempdir().expect("tempdir");
    let expected = std::env::current_dir()
        .expect("working directory")
        .join(".git")
        .join("index");
    assert_eq!(
        owned_index(OsStr::new(".git/index"), git_dir.path(), git_dir.path()).expect("resolves"),
        Some(expected)
    );
}

#[test]
fn an_index_of_this_repository_that_cannot_be_resolved_fails_the_review() {
    let git_dir = tempfile::tempdir().expect("tempdir");
    assert!(owned_index(OsStr::new(""), git_dir.path(), git_dir.path()).is_err());
}
