//! `run_git` answers about the repository drep names, whatever repository the environment it inherited names.

use std::path::Path;

use crate::diff::{run_git, same_directory};
use crate::test_support::git_init;

#[tokio::test]
async fn run_git_answers_about_the_repository_it_is_given() {
    let repository = tempfile::tempdir().expect("tempdir");
    git_init(repository.path());
    let answered = run_git(repository.path(), &["rev-parse", "--absolute-git-dir"])
        .await
        .expect("git answers");
    assert!(
        same_directory(Path::new(&answered), &repository.path().join(".git")),
        "git answered about {answered}"
    );
}

/// Under a hook, drep inherits the hook's `GIT_DIR` and `GIT_INDEX_FILE`; the test above, run with another repository's, must still hold.
#[test]
fn run_git_answers_about_its_repository_under_a_hook_naming_another() {
    let decoy = tempfile::tempdir().expect("tempdir");
    git_init(decoy.path());
    let git_dir = decoy.path().join(".git");
    let output = std::process::Command::new(std::env::current_exe().expect("this test binary"))
        .args([
            "diff::tests::git_env::run_git_answers_about_the_repository_it_is_given",
            "--exact",
            "--test-threads=1",
        ])
        .env("GIT_DIR", &git_dir)
        .env("GIT_INDEX_FILE", git_dir.join("index"))
        .output()
        .expect("run the test under a hook's environment");
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("1 passed"),
        "run_git answered about the hook's repository: {output:?}"
    );
}
