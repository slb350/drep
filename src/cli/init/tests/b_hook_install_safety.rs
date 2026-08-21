//! Hook installation safety and idempotence.

use crate::cli::init::hooks::{HookKind, hook_body, install};

/// `--hooks none` does no filesystem or git work at all.
///
/// Asserting only "no hook file exists" cannot see this: with an empty name
/// list the write loop never runs either way, so deleting the early return
/// passes. The observable difference is everything *around* the loop -
/// locating the git dir, creating the hooks directory, and querying
/// `core.hooksPath`, which reports itself. So the fixture sets a hooks path
/// and asserts total silence: an escape hatch with side effects is not one.
#[tokio::test]
async fn hooks_none_does_no_work_at_all() {
    let dir = tempfile::tempdir().expect("tempdir");
    crate::test_support::git_init(dir.path());

    let shared = dir.path().join("shared-hooks");
    let status = crate::test_support::git(dir.path())
        .args(["config", "--local", "core.hooksPath"])
        .arg(&shared)
        .status()
        .expect("git config");
    assert!(status.success());

    let mut out: Vec<u8> = Vec::new();
    install(&mut out, dir.path(), HookKind::None, false)
        .await
        .expect("install");

    assert!(
        out.is_empty(),
        "nothing was asked for, so nothing should be reported; got:\n{}",
        String::from_utf8_lossy(&out)
    );
    assert!(
        !shared.exists(),
        "and the chainer directory must not be created"
    );
}

/// `--force` over a foreign hook keeps a copy rather than destroying it.
#[tokio::test]
async fn forcing_over_a_foreign_hook_backs_it_up_first() {
    let dir = tempfile::tempdir().expect("tempdir");
    crate::test_support::git_init(dir.path());
    let hooks = dir.path().join(".git/hooks");
    std::fs::create_dir_all(&hooks).expect("hooks dir");
    let foreign = "#!/bin/sh\n# my secret scanner\nexit 0\n";
    std::fs::write(hooks.join("pre-push"), foreign).expect("write foreign");

    let mut out: Vec<u8> = Vec::new();
    install(&mut out, dir.path(), HookKind::PrePush, true)
        .await
        .expect("install");
    let text = String::from_utf8(out).expect("utf8");

    assert_eq!(
        std::fs::read_to_string(hooks.join("pre-push")).expect("read"),
        hook_body("pre-push").expect("known"),
        "--force does replace the hook"
    );
    assert_eq!(
        std::fs::read_to_string(hooks.join("pre-push.drep-backup"))
            .expect("the previous hook must be kept"),
        foreign,
        "and the user's own hook is preserved verbatim"
    );
    assert!(
        text.contains("saved at"),
        "and they are told where; got:\n{text}"
    );
}

/// A hook drep wrote is replaced with no backup, because there is nothing to preserve.
#[tokio::test]
async fn refreshing_dreps_own_hook_leaves_no_backup() {
    let dir = tempfile::tempdir().expect("tempdir");
    crate::test_support::git_init(dir.path());

    install(&mut Vec::new(), dir.path(), HookKind::PrePush, false)
        .await
        .expect("first install");
    install(&mut Vec::new(), dir.path(), HookKind::PrePush, false)
        .await
        .expect("second install");

    let hooks = dir.path().join(".git/hooks");
    assert!(!hooks.join("pre-push.drep-backup").exists());
    assert!(!hooks.join("pre-push.drep-tmp").exists());
}

/// Installing twice leaves the hook executable.
#[tokio::test]
async fn installing_twice_leaves_the_hook_executable() {
    let dir = tempfile::tempdir().expect("tempdir");
    crate::test_support::git_init(dir.path());
    let hook = dir.path().join(".git/hooks/pre-push");

    for pass in 1..=2 {
        install(&mut Vec::new(), dir.path(), HookKind::PrePush, false)
            .await
            .unwrap_or_else(|e| panic!("install pass {pass}: {e}"));
        assert!(
            crate::languages::runner::is_executable(&hook),
            "the hook must be executable after pass {pass}"
        );
    }
}
