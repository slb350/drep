//! The diff tests' fixtures act only on their own repositories, even when a git hook runs the suite with the committing repository's environment.

use crate::test_support::{git_env, git_output};

#[test]
fn diff_fixtures_leave_the_repository_a_hook_runs_them_in_alone() {
    let enclosing = tempfile::tempdir().expect("tempdir");
    git_output(enclosing.path(), &["init", "--quiet"]);
    let git_dir = enclosing.path().join(".git");
    let before = git_env::repository_state(&git_dir);

    let output = std::process::Command::new(std::env::current_exe().expect("this test binary"))
        .args([
            "diff::tests::staged_files::returns_a_staged_added_source_file",
            "--exact",
            "--test-threads=1",
        ])
        .envs(git_env::hook_environment(&git_dir))
        .output()
        .expect("run a diff test under a hook's environment");

    assert!(
        String::from_utf8_lossy(&output.stdout).contains("1 passed"),
        "the child ran its one test cleanly: {output:?}"
    );
    assert_eq!(
        git_env::repository_state(&git_dir),
        before,
        "a fixture changed the enclosing repository"
    );
}
