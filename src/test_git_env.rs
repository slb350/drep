//! What a test fixture's git must not inherit from a hook the suite may run under, and what a fixture leaking into a repository would change.
//!
//! A hook exports the committing repository's `GIT_DIR` and `GIT_INDEX_FILE`, any `-c` configuration and the commit's identity. A fixture's git that inherited them would act on that repository instead of its own: it once set `core.bare`, `core.hooksPath` and a fixture identity in a developer's real `.git/config` and committed fixture files onto a real branch. The unit tests reach this through `crate::test_support`, and the integration tests through `tests/common`, which includes this file by path, so both share one definition.

#![allow(dead_code)]

use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;

/// Removes from `command` everything a fixture's git must not inherit: every variable git calls repository-local (`git rev-parse --local-env-vars`: the repository, its index and object store, and `-c` configuration), the commit identity a hook exports, and a receiving push's quarantine.
pub fn scrub_outer_git(command: &mut Command) -> &mut Command {
    for variable in outer_git_variables() {
        command.env_remove(variable);
    }
    command
}

fn outer_git_variables() -> &'static [String] {
    static VARIABLES: OnceLock<Vec<String>> = OnceLock::new();
    VARIABLES.get_or_init(|| {
        let output = Command::new("git")
            .args(["rev-parse", "--local-env-vars"])
            .output()
            .expect("list git's repository-local environment");
        assert!(output.status.success(), "{output:?}");
        let mut variables: Vec<String> = String::from_utf8(output.stdout)
            .expect("git output is UTF-8")
            .lines()
            .map(str::to_owned)
            .collect();
        variables.extend(
            [
                "GIT_AUTHOR_NAME",
                "GIT_AUTHOR_EMAIL",
                "GIT_AUTHOR_DATE",
                "GIT_COMMITTER_NAME",
                "GIT_COMMITTER_EMAIL",
                "GIT_COMMITTER_DATE",
                "GIT_QUARANTINE_PATH",
            ]
            .map(str::to_owned),
        );
        variables
    })
}

/// What a fixture leaking into the repository at `git_dir` would change: what HEAD names, its configuration, its index and its branches.
pub fn repository_state(git_dir: &Path) -> (Vec<u8>, Vec<u8>, Option<Vec<u8>>, Vec<String>) {
    let mut branches: Vec<String> = std::fs::read_dir(git_dir.join("refs").join("heads"))
        .expect("list the branches")
        .map(|entry| {
            entry
                .expect("read a branch")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    branches.sort();
    (
        std::fs::read(git_dir.join("HEAD")).expect("read HEAD"),
        std::fs::read(git_dir.join("config")).expect("read the configuration"),
        std::fs::read(git_dir.join("index")).ok(),
        branches,
    )
}

/// The environment a hook would give a child test run against the repository at `git_dir`: its `GIT_DIR` and `GIT_INDEX_FILE`, and a `-c` setting that fails every commit inheriting it, by signing through `false`.
pub fn hook_environment(git_dir: &Path) -> [(&'static str, std::ffi::OsString); 3] {
    [
        ("GIT_DIR", git_dir.as_os_str().to_owned()),
        ("GIT_INDEX_FILE", git_dir.join("index").into_os_string()),
        (
            "GIT_CONFIG_PARAMETERS",
            "'commit.gpgsign'='true' 'gpg.program'='false'".into(),
        ),
    ]
}
