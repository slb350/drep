//! Hooks-path aliasing regressions.

use crate::cli::init::hooks::{HookKind, hook_body, install};

async fn assert_repo_hook_is_used(root: &std::path::Path, configured_hooks_path: &str) {
    let status = crate::test_support::git(root)
        .args(["config", "--local", "core.hooksPath", configured_hooks_path])
        .status()
        .expect("set core.hooksPath");
    assert!(status.success());

    let mut out = Vec::new();
    install(&mut out, root, HookKind::PrePush, false)
        .await
        .expect("install");

    let hook = root.join(".git/hooks/pre-push");
    assert_eq!(
        std::fs::read_to_string(&hook).expect("read installed hook"),
        hook_body("pre-push").expect("known hook"),
        "a chainer in the same directory would replace the hook and recurse into itself"
    );
    assert!(
        !String::from_utf8(out)
            .expect("utf8")
            .contains("needs a chainer"),
        "the active hooks directory already contains the repository hook"
    );
}

#[tokio::test]
async fn the_repository_hooks_directory_is_not_replaced_by_a_chainer() {
    let dir = tempfile::tempdir().expect("tempdir");
    crate::test_support::git_init(dir.path());
    assert_repo_hook_is_used(dir.path(), ".git/hooks").await;
}

#[cfg(unix)]
#[tokio::test]
async fn an_alias_of_the_repository_hooks_directory_is_not_replaced_by_a_chainer() {
    let dir = tempfile::tempdir().expect("tempdir");
    crate::test_support::git_init(dir.path());

    let hooks = dir.path().join(".git/hooks");
    let alias = dir.path().join("hooks-alias");
    std::os::unix::fs::symlink(&hooks, &alias).expect("symlink hooks directory");

    assert_repo_hook_is_used(dir.path(), alias.to_str().expect("UTF-8 alias path")).await;
}
