//! B8, B9, B10-B16: hooks.rs.
//!
//! The git-touching criteria need a real `git init` repository; the helper
//! at the bottom builds one with `user.email`/`user.name` set so git can
//! answer queries.

use std::path::Path;

use crate::cli::init::hooks::{
    HookKind, chainer_body, hook_body, hook_names, install, is_drep_managed,
};

#[test]
fn hook_names_match_spec_for_every_kind() {
    assert_eq!(hook_names(HookKind::None), &[] as &[&str]);
    assert_eq!(hook_names(HookKind::PrePush), &["pre-push"]);
    assert_eq!(hook_names(HookKind::PreCommit), &["pre-commit"]);
    assert_eq!(hook_names(HookKind::Both), &["pre-commit", "pre-push"]);
}

#[test]
fn is_drep_managed_recognises_own_bodies_only() {
    assert!(is_drep_managed(hook_body("pre-commit").unwrap()));
    assert!(is_drep_managed(hook_body("pre-push").unwrap()));
    assert!(is_drep_managed(&chainer_body("pre-push")));
    assert!(!is_drep_managed("#!/bin/sh\necho hi\n"));
}

fn hooks_dir(root: &Path) -> std::path::PathBuf {
    root.join(".git/hooks")
}

#[tokio::test]
async fn install_writes_executable_repo_local_pre_push_hook() {
    let dir = tempfile::tempdir().expect("tempdir");
    crate::test_support::git_init(dir.path());

    let mut out = Vec::new();
    let result = install(&mut out, dir.path(), HookKind::PrePush, false).await;
    assert!(result.is_ok(), "install: {result:?}");

    let hook_path = hooks_dir(dir.path()).join("pre-push");
    let written = std::fs::read_to_string(&hook_path).expect("read hook");
    assert_eq!(
        written,
        hook_body("pre-push").expect("known"),
        "the installed hook's body equals the canonical pre-push body"
    );

    crate::test_support::assert_executable(&hook_path);
}

#[tokio::test]
async fn install_does_not_clobber_a_foreign_hook_without_force() {
    let dir = tempfile::tempdir().expect("tempdir");
    crate::test_support::git_init(dir.path());
    let foreign = "#!/bin/sh\necho mine\n";
    let hook_path = hooks_dir(dir.path()).join("pre-push");
    std::fs::write(&hook_path, foreign).expect("write foreign");

    let mut out = Vec::new();
    let result = install(&mut out, dir.path(), HookKind::PrePush, false).await;
    assert!(result.is_ok(), "install returned Ok for a foreign hook");
    let after = std::fs::read_to_string(&hook_path).expect("read");
    assert_eq!(after, foreign, "foreign hook is byte-for-byte unchanged");

    let rendered = String::from_utf8(out).expect("utf8");
    assert!(
        rendered.contains("--force"),
        "captured output mentions --force; got:\n{rendered}"
    );

    // Now force: the file is replaced.
    let mut out = Vec::new();
    let result = install(&mut out, dir.path(), HookKind::PrePush, true).await;
    assert!(result.is_ok(), "install with force");
    let after = std::fs::read_to_string(&hook_path).expect("read");
    assert_eq!(
        after,
        hook_body("pre-push").expect("known"),
        "force replaces the foreign hook"
    );
}

#[tokio::test]
async fn install_rewrites_its_own_modified_hook() {
    let dir = tempfile::tempdir().expect("tempdir");
    crate::test_support::git_init(dir.path());
    let hook_path = hooks_dir(dir.path()).join("pre-push");
    // Keep the marker, change a comment.
    let modified = "# Managed by `drep init`.\n# an older version\n";
    std::fs::write(&hook_path, modified).expect("write modified");

    let mut out = Vec::new();
    install(&mut out, dir.path(), HookKind::PrePush, false)
        .await
        .expect("install");

    let after = std::fs::read_to_string(&hook_path).expect("read");
    assert_eq!(
        after,
        hook_body("pre-push").expect("known"),
        "a drep-managed hook is restored even without --force"
    );
}

#[test]
fn resolve_hooks_dir_handles_relative_and_absolute() {
    // root is intentionally NOT the process cwd: an implementation that
    // resolved against cwd would write a chainer into /tmp or wherever.
    let root = Path::new("/srv/some/repo");
    assert_eq!(
        crate::cli::init::hooks::resolve_hooks_dir(root, "/etc/hooks"),
        Path::new("/etc/hooks").to_path_buf()
    );
    assert_eq!(
        crate::cli::init::hooks::resolve_hooks_dir(root, "shared/hooks"),
        Path::new("/srv/some/repo/shared/hooks").to_path_buf()
    );
}

#[tokio::test]
async fn install_writes_chainer_when_core_hooks_path_is_set() {
    let dir = tempfile::tempdir().expect("tempdir");
    crate::test_support::git_init(dir.path());
    let rel = "shared/hooks";
    let status = crate::test_support::git(dir.path())
        .args(["config", "--local", "core.hooksPath", rel])
        .status()
        .expect("set core.hooksPath");
    assert!(status.success());

    let mut out = Vec::new();
    install(&mut out, dir.path(), HookKind::PrePush, false)
        .await
        .expect("install");

    let rendered = String::from_utf8(out).expect("utf8");
    assert!(
        rendered.contains("core.hooksPath is set to"),
        "rendered:\n{rendered}"
    );

    // Repo-local hook was written too.
    let local = hooks_dir(dir.path()).join("pre-push");
    assert!(local.exists(), "repo-local pre-push must exist");
    assert_eq!(
        std::fs::read_to_string(&local).expect("read local"),
        hook_body("pre-push").expect("known")
    );

    // Chainer was written in the resolved hooks dir.
    let chainer = dir.path().join(rel).join("pre-push");
    assert!(chainer.exists(), "chainer must exist at {chainer:?}");
    let body = std::fs::read_to_string(&chainer).expect("read chainer");
    assert!(
        body.contains("hooks/pre-push"),
        "chainer forwards to the repo-local hook: {body}"
    );
    crate::test_support::assert_executable(&chainer);
}

#[tokio::test]
async fn install_leaves_a_foreign_chainer_alone() {
    let dir = tempfile::tempdir().expect("tempdir");
    crate::test_support::git_init(dir.path());
    let rel = "shared/hooks";
    crate::test_support::git(dir.path())
        .args(["config", "--local", "core.hooksPath", rel])
        .status()
        .expect("set core.hooksPath");

    // Write the repo-local hook so the foreign chainer doesn't get rewritten.
    std::fs::create_dir_all(hooks_dir(dir.path())).expect("hooks dir");
    std::fs::write(
        hooks_dir(dir.path()).join("pre-push"),
        hook_body("pre-push").expect("known"),
    )
    .expect("write local");

    // Foreign chainer.
    let chainer_dir = dir.path().join(rel);
    std::fs::create_dir_all(&chainer_dir).expect("chainer dir");
    let foreign = "#!/bin/sh\necho other\n";
    crate::test_support::write_executable(&chainer_dir.join("pre-push"), foreign);

    let mut out = Vec::new();
    install(&mut out, dir.path(), HookKind::PrePush, false)
        .await
        .expect("install");
    let rendered = String::from_utf8(out).expect("utf8");
    assert!(
        rendered.contains("does not appear to chain"),
        "foreign chainer must be reported; rendered:\n{rendered}"
    );
    let after = std::fs::read_to_string(chainer_dir.join("pre-push")).expect("read");
    assert_eq!(after, foreign, "foreign chainer is untouched");
}

/// In a **linked worktree**, the hook must land in the main repository's
/// `.git/hooks`, not in the worktree's own git directory.
///
/// This is the entire reason the installer asks git for `--git-common-dir`
/// rather than `--git-dir` (or, worse, joining `root/.git`). In an ordinary
/// checkout the two answers are identical, so every other test in this file
/// passes just as well against the wrong one — this is the only shape that
/// tells them apart. In a linked worktree `.git` is a *file*, `--git-dir`
/// points at `.git/worktrees/<name>`, and git runs hooks from the common dir:
/// install into the wrong one and the hook silently never fires.
#[tokio::test]
async fn a_linked_worktree_installs_into_the_main_repositorys_hooks_dir() {
    let dir = tempfile::tempdir().expect("tempdir");
    let main = dir.path().join("main");
    std::fs::create_dir_all(&main).expect("main dir");
    crate::test_support::git_init(&main);

    // A worktree needs a commit to branch from.
    std::fs::write(main.join("seed.txt"), "seed\n").expect("seed");
    for args in [
        vec!["add", "seed.txt"],
        vec!["commit", "--quiet", "-m", "root"],
    ] {
        let status = crate::test_support::git(&main)
            .args(&args)
            .status()
            .expect("git");
        assert!(status.success(), "git {args:?} failed");
    }

    let linked = dir.path().join("linked");
    let output = crate::test_support::git(&main)
        .args(["worktree", "add", "-b", "side"])
        .arg(&linked)
        .output()
        .expect("git worktree add");
    assert!(
        output.status.success(),
        "git worktree add failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        linked.join(".git").is_file(),
        "a linked worktree's .git is a file, which is the premise of this test"
    );

    let mut out: Vec<u8> = Vec::new();
    install(&mut out, &linked, HookKind::PrePush, false)
        .await
        .expect("install into a linked worktree");

    let shared = main.join(".git").join("hooks").join("pre-push");
    assert!(
        shared.is_file(),
        "the hook belongs in the main repository's hooks dir; output was:\n{}",
        String::from_utf8_lossy(&out)
    );
    assert_eq!(
        std::fs::read_to_string(&shared).expect("read hook"),
        hook_body("pre-push").expect("known"),
        "and it must be drep's body"
    );

    // The per-worktree git dir must NOT have received it: git does not run
    // hooks from there, so a hook written there is a hook that never fires.
    let per_worktree = main
        .join(".git")
        .join("worktrees")
        .join("linked")
        .join("hooks")
        .join("pre-push");
    assert!(
        !per_worktree.exists(),
        "the hook must not be written to the per-worktree git dir at {}",
        per_worktree.display()
    );
}

/// An **empty** `core.hooksPath` means "hooks are disabled", not "hooks live
/// in the current directory".
///
/// git reads an empty value back as present-but-blank, and the 1.x installer
/// documented this as one of its two silent failure modes. Treating it as a
/// directory resolves to `root.join("")`, i.e. the repository root, so drep
/// would scatter chainers next to the user's source files and report a
/// `core.hooksPath is set to` line naming nothing.
#[tokio::test]
async fn an_empty_core_hooks_path_is_treated_as_unset() {
    let dir = tempfile::tempdir().expect("tempdir");
    crate::test_support::git_init(dir.path());
    // `git_init` already sets it empty; state it here so the test does not
    // silently depend on that and still means what its name says.
    let status = crate::test_support::git(dir.path())
        .args(["config", "--local", "core.hooksPath", ""])
        .status()
        .expect("git config");
    assert!(status.success());

    let mut out: Vec<u8> = Vec::new();
    install(&mut out, dir.path(), HookKind::PrePush, false)
        .await
        .expect("install");
    let text = String::from_utf8(out).expect("utf8");

    assert!(
        !text.contains("core.hooksPath is set to"),
        "an empty value is not a hooks path; got:\n{text}"
    );
    assert!(
        !dir.path().join("pre-push").exists(),
        "no chainer may be written into the repository root"
    );
    assert!(
        dir.path()
            .join(".git")
            .join("hooks")
            .join("pre-push")
            .is_file(),
        "the repo-local hook is still installed"
    );
}

/// An existing chainer that chains but is **not executable** is made
/// executable; one that already is, is left untouched.
///
/// git ignores a non-executable hook without saying anything, so this branch
/// is the difference between "drep runs on push" and "drep silently never
/// runs, and nothing tells you". Both halves are asserted in one test because
/// either alone leaves the condition free to invert: without the second, a
/// version that chmods unconditionally passes, and it is the *unconditional*
/// version that rewrites file modes the user set deliberately.
#[tokio::test]
async fn a_chainer_that_is_not_executable_is_fixed_and_an_executable_one_is_left_alone() {
    let dir = tempfile::tempdir().expect("tempdir");
    crate::test_support::git_init(dir.path());

    let shared = dir.path().join("shared-hooks");
    std::fs::create_dir_all(&shared).expect("shared hooks dir");
    let status = crate::test_support::git(dir.path())
        .args(["config", "--local", "core.hooksPath"])
        .arg(&shared)
        .status()
        .expect("git config");
    assert!(status.success());

    // A chainer that chains correctly, but with the executable bit cleared.
    let chainer = shared.join("pre-push");
    std::fs::write(&chainer, chainer_body("pre-push")).expect("write chainer");
    crate::test_support::clear_executable(&chainer);

    let mut out: Vec<u8> = Vec::new();
    install(&mut out, dir.path(), HookKind::PrePush, false)
        .await
        .expect("install");
    let text = String::from_utf8(out).expect("utf8");

    crate::test_support::assert_executable(&chainer);
    assert!(
        text.contains("is not executable; making it so"),
        "and the fix must be reported; got:\n{text}"
    );

    // Second run: it is executable now, so drep has nothing to say about it -
    // and, critically, must not take the bit back off. Re-applying the mode to
    // an already-executable file is the only situation that can tell OR from
    // XOR, and XOR here would silently disable the chainer on every second
    // `drep init`.
    let mut out: Vec<u8> = Vec::new();
    install(&mut out, dir.path(), HookKind::PrePush, false)
        .await
        .expect("install again");
    let text = String::from_utf8(out).expect("utf8");
    assert!(
        !text.contains("is not executable"),
        "an already-executable chainer needs no announcement; got:\n{text}"
    );
    crate::test_support::assert_executable(&chainer);
}

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
///
/// `--force` serves two destinations, and `config_file::write` is what tells
/// the user to reach for it ("Re-run with --force to replace it") - so someone
/// with an existing `drep.toml` and a hand-written pre-push hook is steered
/// straight into losing the hook. The backup is what makes that recoverable.
#[tokio::test]
async fn forcing_over_a_foreign_hook_backs_it_up_first() {
    let dir = tempfile::tempdir().expect("tempdir");
    crate::test_support::git_init(dir.path());
    let hooks = dir.path().join(".git").join("hooks");
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
    let backup = hooks.join("pre-push.drep-backup");
    assert_eq!(
        std::fs::read_to_string(&backup).expect("the previous hook must be kept"),
        foreign,
        "and the user's own hook is preserved verbatim"
    );
    assert!(
        text.contains("saved at"),
        "and they are told where; got:\n{text}"
    );
}

/// A hook drep wrote is replaced with no backup, because there is nothing to
/// preserve.
///
/// The counterpart to the test above: without it, "always write a backup"
/// would pass, littering `.git/hooks` with a stale copy on every run.
#[tokio::test]
async fn refreshing_dreps_own_hook_leaves_no_backup() {
    let dir = tempfile::tempdir().expect("tempdir");
    crate::test_support::git_init(dir.path());

    let mut out: Vec<u8> = Vec::new();
    install(&mut out, dir.path(), HookKind::PrePush, false)
        .await
        .expect("first install");
    let mut out: Vec<u8> = Vec::new();
    install(&mut out, dir.path(), HookKind::PrePush, false)
        .await
        .expect("second install");

    let hooks = dir.path().join(".git").join("hooks");
    assert!(
        !hooks.join("pre-push.drep-backup").exists(),
        "drep's own hook needs no backup"
    );
    assert!(
        !hooks.join("pre-push.drep-tmp").exists(),
        "and the atomic-write temp file must not be left behind"
    );
}

/// Installing twice leaves the hook executable.
///
/// `set_executable` ORs the execute bits in; XOR-ing them would *toggle*, so a
/// second install would silently strip the bits off a hook that was already
/// executable - and git ignores a non-executable hook without a word, so the
/// gate would simply stop running. Only a second install can see this.
#[tokio::test]
async fn installing_twice_leaves_the_hook_executable() {
    let dir = tempfile::tempdir().expect("tempdir");
    crate::test_support::git_init(dir.path());
    let hook = dir.path().join(".git").join("hooks").join("pre-push");

    for pass in 1..=2 {
        let mut out: Vec<u8> = Vec::new();
        install(&mut out, dir.path(), HookKind::PrePush, false)
            .await
            .unwrap_or_else(|e| panic!("install pass {pass}: {e}"));
        assert!(
            crate::languages::runner::is_executable(&hook),
            "the hook must be executable after pass {pass}"
        );
    }
}
