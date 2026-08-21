//! B16: the pre-push hook body actually invokes `drep check --diff <oid>`.
//!
//! This is the only criterion that proves the shell in the hook is correct
//! rather than merely present. The test runs the hook under `sh` with a
//! stub `drep` on PATH (set on the *child* process only, never on the test
//! process) and asserts the recorded arguments.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::cli::init::hooks::hook_body;

/// All-zero oid, as git sends for a ref that does not exist on the far side.
const ZEROS: &str = "0000000000000000000000000000000000000000";

/// The ordinary case: the branch exists upstream, so git sends its current
/// remote oid and that is exactly what `--diff` should receive.
#[test]
fn pre_push_hook_invokes_drep_check_diff_with_the_remote_oid() {
    let dir = tempfile::tempdir().expect("tempdir");
    crate::test_support::git_init(dir.path());

    let local_oid = "1111111111111111111111111111111111111111";
    let remote_oid = "2222222222222222222222222222222222222222";
    let stdin = format!("refs/heads/x {local_oid} refs/heads/x {remote_oid}\n");
    // `false`: this path never shells out to git, so withholding the real PATH
    // proves it - if the implementation grew a git call here, the hook would
    // fail and this test would catch it.
    let recorded = run_pre_push(dir.path(), &stdin, false);

    assert_eq!(
        recorded,
        vec![
            "check",
            "--push-gate",
            "--diff",
            remote_oid,
            "--tip",
            local_oid,
        ],
        "the remote oid is the base, and the *pushed* oid is the tip - not HEAD"
    );
}

/// Write the stub `drep` and the hook, run the hook with `stdin`, and return
/// the arguments the stub recorded.
///
/// `PATH` is set on the **child** only. Mutating the test process's
/// environment is `unsafe` in edition 2024 and races every other test in the
/// binary - several of which spawn `git`, which reads `PATH` to find itself.
fn run_pre_push(dir: &Path, stdin: &str, include_real_path: bool) -> Vec<String> {
    let (status, recorded) = run_pre_push_status(dir, stdin, include_real_path, 0);
    assert_eq!(
        status,
        Some(0),
        "hook should succeed; recorded: {recorded:?}"
    );
    recorded
}

/// `run_pre_push`, keeping the hook's exit status and letting the stub `drep`
/// exit with `stub_exit`.
///
/// The status is the whole point of a pre-push hook - it is what aborts the
/// push - and the original harness asserted success unconditionally, so no
/// test could observe it. A hook body of `drep check … || true; exit 0` passed
/// every test in this file.
fn run_pre_push_status(
    dir: &Path,
    stdin: &str,
    include_real_path: bool,
    stub_exit: i32,
) -> (Option<i32>, Vec<String>) {
    run_pre_push_with_stub(
        dir,
        stdin,
        include_real_path,
        &format!("exit {stub_exit}\n"),
    )
}

/// Install a `drep` stub that records argv before running `stub_body`.
fn recording_stub(dir: &Path, stub_body: &str) -> (PathBuf, PathBuf) {
    let bin_dir = dir.join("bin");
    std::fs::create_dir_all(&bin_dir).expect("bin dir");
    let args_log = dir.join("args.log");
    let stub = format!(
        "#!/bin/sh\nfor a in \"$@\"; do printf '%s\\n' \"$a\" >> \
         \"$DREP_TEST_ARGS_LOG\"; done\n{stub_body}"
    );
    crate::test_support::write_executable(&bin_dir.join("drep"), stub);
    (bin_dir, args_log)
}

/// The one pre-push execution fixture. `stub_body` runs after every argument
/// has been logged, so a test can vary provider status without duplicating the
/// hook, PATH, stdin, or child-process plumbing.
fn run_pre_push_with_stub(
    dir: &Path,
    stdin: &str,
    include_real_path: bool,
    stub_body: &str,
) -> (Option<i32>, Vec<String>) {
    let (bin_dir, args_log) = recording_stub(dir, stub_body);

    let hook_path = dir.join("pre-push");
    crate::test_support::write_executable(&hook_path, hook_body("pre-push").expect("known"));

    // The zero-oid branch shells out to `git`, so that case needs the real
    // PATH as well as the stub directory.
    let mut entries: Vec<PathBuf> = vec![bin_dir.clone()];
    if include_real_path && let Some(existing) = std::env::var_os("PATH") {
        entries.extend(std::env::split_paths(&existing));
    }
    let path = std::env::join_paths(entries).expect("join paths");

    let mut child = Command::new("/bin/sh")
        .arg(&hook_path)
        .env("PATH", &path)
        .env("DREP_TEST_ARGS_LOG", &args_log)
        .env("DREP_TEST_COUNTER", dir.join("count"))
        .current_dir(dir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn hook");
    use std::io::Write;
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(stdin.as_bytes())
        .expect("write stdin");
    let status = child.wait().expect("wait hook");
    let recorded: Vec<String> = std::fs::read_to_string(&args_log)
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect();
    (status.code(), recorded)
}

/// A branch that does not exist upstream yet sends an all-zero remote oid.
/// The hook must fall back to a real base rather than passing the zeros
/// through as a git ref.
///
/// This is the common case for the *first* push of a branch, and the path the
/// happy-path test above never reaches. Passing `0000...` to
/// `drep check --diff` would make git fail to resolve the ref, and the hook
/// would abort every first push.
#[test]
fn a_new_branch_falls_back_to_a_real_base_rather_than_the_zero_oid() {
    let dir = tempfile::tempdir().expect("tempdir");
    crate::test_support::git_init(dir.path());

    std::fs::write(dir.path().join("seed.txt"), "seed\n").expect("seed");
    for args in [
        vec!["add", "seed.txt"],
        vec!["commit", "--quiet", "-m", "root"],
    ] {
        let status = crate::test_support::git(dir.path())
            .args(&args)
            .status()
            .expect("git");
        assert!(status.success(), "git {args:?} failed");
    }
    let head = String::from_utf8(
        crate::test_support::git(dir.path())
            .args(["rev-parse", "HEAD"])
            .output()
            .expect("rev-parse")
            .stdout,
    )
    .expect("utf8");
    let head = head.trim().to_owned();

    let stdin = format!("refs/heads/x {head} refs/heads/x {ZEROS}\n");
    let recorded = run_pre_push(dir.path(), &stdin, true);

    assert_eq!(
        recorded.first().map(String::as_str),
        Some("check"),
        "recorded: {recorded:?}"
    );
    assert_eq!(recorded.get(1).map(String::as_str), Some("--push-gate"));
    assert_eq!(recorded.get(2).map(String::as_str), Some("--diff"));
    assert_eq!(
        recorded.get(4).map(String::as_str),
        Some("--tip"),
        "recorded: {recorded:?}"
    );
    assert_eq!(
        recorded.get(5).map(String::as_str),
        Some(head.as_str()),
        "the tip is the ref being pushed"
    );
    let base = recorded.get(3).expect("a base ref");
    assert_ne!(
        base, ZEROS,
        "the all-zero oid must never be passed through as a git ref"
    );
    assert!(
        !base.is_empty(),
        "the fallback must produce a base, got an empty ref"
    );
    // With no `origin/HEAD` in a fresh repo the fallback is the root commit,
    // which for a single-commit repo is HEAD itself.
    assert_eq!(base, &head, "recorded: {recorded:?}");
}

/// A branch *deletion* sends an all-zero local oid. There is no content to
/// review, so the hook must not call drep at all - passing a deleted ref to
/// `--diff` would fail the push for a change that removes code.
#[test]
fn a_branch_deletion_does_not_invoke_drep() {
    let dir = tempfile::tempdir().expect("tempdir");
    crate::test_support::git_init(dir.path());

    let stdin = format!("(delete) {ZEROS} refs/heads/x 3333333333333333333333333333333333333333\n");
    let recorded = run_pre_push(dir.path(), &stdin, true);

    assert!(
        recorded.is_empty(),
        "a deletion has nothing to review; drep must not run. recorded: {recorded:?}"
    );
}

/// The hook's exit status is drep's, so a failing check aborts the push.
///
/// This is the hook's entire reason for existing, and nothing observed it: a
/// body ending `drep check … || true` then `exit 0` passed every other test
/// here. Both exit codes are checked because they mean different things
/// downstream - 1 is "found issues", 2 is "could not analyze", and 3 is
/// "review cached; reconnect and push again".
#[test]
fn a_failing_check_aborts_the_push() {
    for (drep_exit, hook_exit) in [(1, 1), (2, 2), (3, 3), (42, 2)] {
        let dir = tempfile::tempdir().expect("tempdir");
        crate::test_support::git_init(dir.path());
        let stdin = "refs/heads/x 1111111111111111111111111111111111111111 refs/heads/x \
                     2222222222222222222222222222222222222222\n";
        let (status, recorded) = run_pre_push_status(dir.path(), stdin, false, drep_exit);
        assert_eq!(
            status,
            Some(hook_exit),
            "drep exited {drep_exit}, so the hook must exit {hook_exit}; recorded: {recorded:?}"
        );
    }
}

/// With several refs, failure severity wins rather than numeric or temporal
/// order: could-not-analyze (2), findings (1), reconnect (3), clean (0).
///
/// Exit 3 is numerically highest but semantically lowest among the stopping
/// statuses: it says the review succeeded and only Git's connection is stale.
/// Last-writer-wins and numeric-max both lose information, so the cases pin
/// the whole ordering.
#[test]
fn failure_precedence_across_refs_is_semantic_not_numeric() {
    for (first, second, expected) in [(2, 1, 2), (2, 3, 2), (1, 3, 1)] {
        let dir = tempfile::tempdir().expect("tempdir");
        crate::test_support::git_init(dir.path());
        let counter = dir.path().join("count");
        let stub_body = format!(
            "if [ -f \"$DREP_TEST_COUNTER\" ]; then exit {second}; \
             else : > \"$DREP_TEST_COUNTER\"; exit {first}; fi\n"
        );
        let stdin = "refs/heads/a 1111111111111111111111111111111111111111 refs/heads/a \
                     2222222222222222222222222222222222222222\n\
                     refs/heads/b 3333333333333333333333333333333333333333 refs/heads/b \
                     4444444444444444444444444444444444444444\n";
        let (status, recorded) = run_pre_push_with_stub(dir.path(), stdin, false, &stub_body);

        assert!(counter.exists(), "both refs must be processed");
        assert_eq!(
            recorded.iter().filter(|arg| *arg == "check").count(),
            2,
            "both refs must invoke drep: {recorded:?}"
        );
        assert_eq!(
            status,
            Some(expected),
            "statuses {first}, {second} must resolve to {expected}"
        );
    }
}

/// `drep` missing from `PATH` blocks rather than letting the push through.
///
/// GUI git clients run hooks with a minimal `PATH` that typically excludes
/// `~/.cargo/bin`, so this is a routine situation rather than a broken
/// install - and the honest behaviour is to block with an explanation.
#[test]
fn a_missing_drep_binary_blocks_the_push_with_an_explanation() {
    let dir = tempfile::tempdir().expect("tempdir");
    crate::test_support::git_init(dir.path());
    let hook_path = dir.path().join("pre-push");
    crate::test_support::write_executable(&hook_path, hook_body("pre-push").expect("known"));

    // An empty PATH: no stub, no real drep.
    let empty = dir.path().join("empty-bin");
    std::fs::create_dir_all(&empty).expect("empty bin");
    let path = std::env::join_paths([empty.as_path()]).expect("join paths");
    let output = Command::new("/bin/sh")
        .arg(&hook_path)
        .env("PATH", &path)
        .current_dir(dir.path())
        .stdin(std::process::Stdio::null())
        .output()
        .expect("run hook");

    assert_ne!(
        output.status.code(),
        Some(0),
        "a missing drep must not silently pass the push"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("not found on PATH"),
        "and it must say why; stderr: {stderr}"
    );
}

/// Run the `pre-commit` body against a stub `drep` that records its argv and
/// exits `stub_exit`, and hand back (status, recorded args).
fn run_pre_commit(dir: &Path, stub_exit: i32) -> (Option<i32>, Vec<String>) {
    let (bin_dir, args_log) = recording_stub(dir, &format!("exit {stub_exit}\n"));

    let hook_path = dir.join("pre-commit");
    crate::test_support::write_executable(&hook_path, hook_body("pre-commit").expect("known"));

    let path = std::env::join_paths([bin_dir]).expect("join paths");
    let output = Command::new("/bin/sh")
        .arg(&hook_path)
        .env("PATH", &path)
        .env("DREP_TEST_ARGS_LOG", &args_log)
        .current_dir(dir)
        .output()
        .expect("run hook");
    let recorded: Vec<String> = std::fs::read_to_string(&args_log)
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect();
    (output.status.code(), recorded)
}

/// The installed hook runs the markdown checks, and runs them first.
///
/// `drep init` used to write `exec drep check --staged` and nothing else, so a
/// repository gated by drep's own installer had no markdown gating at all -
/// invisible in this repository, which ran `lint-docs` from its own
/// `.pre-commit-config.yaml` instead of from the hook it ships.
///
/// First because it is rule-based and takes ~10 ms, and `check` sends files to
/// an LLM: an obvious documentation defect should not cost a round trip.
#[test]
fn the_pre_commit_hook_lints_markdown_before_it_calls_the_llm() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (status, args) = run_pre_commit(dir.path(), 0);

    assert_eq!(status, Some(0), "a passing hook must exit 0");
    let lint = args
        .iter()
        .position(|a| a == "lint-docs")
        .unwrap_or_else(|| panic!("hook must run lint-docs, recorded {args:?}"));
    let check = args
        .iter()
        .position(|a| a == "check")
        .unwrap_or_else(|| panic!("hook must run check, recorded {args:?}"));
    assert!(lint < check, "lint-docs must run first, recorded {args:?}");
    assert!(
        args.contains(&"--staged".to_owned()),
        "both commands take --staged, recorded {args:?}"
    );
}

/// `--fail-on error`, not `--strict`.
///
/// Under the Phase 6 severity scale `--strict` blocks on any finding, which
/// over a real repository is dominated by line length and trailing whitespace.
/// A hook that blocks a commit over a long line is a hook that gets deleted.
/// `error` is exactly one check - an unclosed fence, which turns the rest of
/// the document into code.
#[test]
fn the_pre_commit_hook_blocks_only_on_error_severity_markdown() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (_, args) = run_pre_commit(dir.path(), 0);
    let fail_on = args
        .iter()
        .position(|a| a == "--fail-on")
        .unwrap_or_else(|| panic!("expected --fail-on, recorded {args:?}"));
    assert_eq!(args.get(fail_on + 1).map(String::as_str), Some("error"));
    assert!(
        !args.iter().any(|a| a == "--strict"),
        "--strict blocks on info findings, recorded {args:?}"
    );
}

/// A failing markdown lint aborts the commit without paying for the LLM.
#[test]
fn a_failed_markdown_lint_stops_the_hook_before_check_runs() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (status, args) = run_pre_commit(dir.path(), 3);

    assert_eq!(status, Some(3), "the hook must propagate the exit status");
    assert!(
        !args.iter().any(|a| a == "check"),
        "check must not run after lint-docs failed, recorded {args:?}"
    );
}
