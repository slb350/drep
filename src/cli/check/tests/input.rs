//! Input resolution: criteria 4-8.
//!
//! Five things the input layer owns end-to-end:
//!
//! - Paths mode produces a single whole-file hunk that covers every line
//!   of the file (criterion 4). Whole-file mode is what makes
//!   `drep check lib.py` see the file the way `drep check --diff main` does:
//!   by line number.
//! - A file drep declined to read is a `FailureReason`, not a silent skip.
//!   Both shapes (oversize, non-UTF-8) are pinned here.
//! - A bad ref under `--diff` is a hard error. "No files changed" and "I
//!   could not ask git" must remain distinct.
//! - A ref that starts with `-` is rejected before any git invocation.
//!   Without the guard, `drep check --diff --something` would reach git as
//!   a flag, and `--` would not save it - git treats arguments after `--`
//!   as paths, not refs.
//!
//! The tests build a `TempDir` and call `input::resolve` directly, so the
//! assertion can reach the `Work` value rather than only its rendered
//! downstream effect.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;

use crate::analysis::result::FailureReason;
use crate::cli::OutputFormat;
use crate::cli::check::CheckArgs;
use crate::cli::check::input::{PreCommitPush, READ_MAX_BYTES};
use crate::cli::check::input::{resolve, resolve_pre_commit};
use crate::diff::hunks::HunkLine;

/// Build a `CheckArgs` for paths mode. Defaults `format = Text`, no
/// `--fail-on`, no `--diff`/`--staged`.
fn paths_args(paths: Vec<PathBuf>) -> CheckArgs {
    CheckArgs {
        paths,
        staged: false,
        diff: None,
        tip: None,
        pre_commit_push: false,
        format: OutputFormat::Text,
        fail_on: None,
        cache_only: false,
        push_gate: false,
    }
}

/// Build a `CheckArgs` for `--diff <ref>` mode.
fn diff_args(ref_: &str) -> CheckArgs {
    CheckArgs {
        paths: Vec::new(),
        staged: false,
        diff: Some(ref_.to_owned()),
        tip: None,
        pre_commit_push: false,
        format: OutputFormat::Text,
        fail_on: None,
        cache_only: false,
        push_gate: false,
    }
}

/// The published hook must resolve the exact range pre-commit derived from
/// git's pre-push stdin. If it falls through to paths mode, the hunk contains
/// only `Context` and drep reviews the whole file again after every fix.
#[tokio::test]
async fn pre_commit_push_refs_resolve_to_diff_hunks() {
    let dir = tempfile::tempdir().expect("tempdir");
    init_repo_with_commit(dir.path());
    let source = dir.path().join("app.py");
    std::fs::write(&source, "value = 1\nunchanged = True\n").expect("base source");
    git_commit(dir.path(), "base", &["app.py"]);
    let base = git_output(dir.path(), &["rev-parse", "HEAD"]);

    std::fs::write(&source, "value = 2\nunchanged = True\n").expect("tip source");
    git_commit(dir.path(), "tip", &["app.py"]);
    let tip = git_output(dir.path(), &["rev-parse", "HEAD"]);
    let context = PreCommitPush::Range {
        from: base,
        to: tip,
    };

    let work = resolve_pre_commit(dir.path(), &context)
        .await
        .expect("pre-commit push range resolves");

    assert_eq!(work.by_file.len(), 1);
    assert_eq!(work.by_file[0][0].file_path, PathBuf::from("app.py"));
    assert!(
        work.by_file[0][0]
            .lines
            .iter()
            .any(|line| matches!(line, HunkLine::Added(text) if text == "value = 2")),
        "the pushed edit must remain marked as an added diff line"
    );
}

/// pre-commit omits FROM/TO for a new branch whose pushed history reaches a
/// root commit and marks the run as all-files. That case must deliberately use
/// whole-tree mode rather than fail or invent a base ref.
#[tokio::test]
async fn pre_commit_new_root_branch_resolves_all_files() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("app.py"), "value = 1\n").expect("source");

    let work = resolve_pre_commit(dir.path(), &PreCommitPush::AllFiles)
        .await
        .expect("all-files push resolves");

    assert_eq!(work.by_file.len(), 1);
    assert!(
        work.by_file[0][0]
            .lines
            .iter()
            .all(|line| matches!(line, HunkLine::Context(_))),
        "pre-commit's explicit all-files case is whole-tree review"
    );
}

/// Parse pre-commit's environment through an injected lookup so the suite
/// never mutates process-global environment while other tests run.
#[test]
fn pre_commit_push_environment_is_complete_or_rejected() {
    let parse = |pairs: &[(&str, &str)]| {
        let values: BTreeMap<&str, OsString> = pairs
            .iter()
            .map(|(key, value)| (*key, OsString::from(value)))
            .collect();
        PreCommitPush::from_lookup(|name| values.get(name).cloned())
    };

    assert!(
        parse(&[]).is_err(),
        "manual use outside pre-commit must fail"
    );
    assert!(
        parse(&[
            ("PRE_COMMIT", "1"),
            ("PRE_COMMIT_REMOTE_NAME", "origin"),
            ("PRE_COMMIT_FROM_REF", "base"),
        ])
        .is_err(),
        "a half-specified range must never fall back to whole-tree mode"
    );
    for (from, to) in [("", "tip"), ("base", ""), ("", "")] {
        assert!(
            parse(&[
                ("PRE_COMMIT", "1"),
                ("PRE_COMMIT_FROM_REF", from),
                ("PRE_COMMIT_TO_REF", to),
            ])
            .is_err(),
            "empty refs are not a valid push range: from={from:?}, to={to:?}"
        );
    }
    assert_eq!(
        parse(&[
            ("PRE_COMMIT", "1"),
            ("PRE_COMMIT_FROM_REF", "base"),
            ("PRE_COMMIT_TO_REF", "tip"),
        ])
        .expect("a complete range does not need remote metadata"),
        PreCommitPush::Range {
            from: "base".to_owned(),
            to: "tip".to_owned(),
        }
    );
    assert_eq!(
        parse(&[
            ("PRE_COMMIT", "1"),
            ("PRE_COMMIT_ORIGIN", "legacy-base"),
            ("PRE_COMMIT_SOURCE", "legacy-tip"),
        ])
        .expect("legacy aliases remain supported"),
        PreCommitPush::Range {
            from: "legacy-base".to_owned(),
            to: "legacy-tip".to_owned(),
        }
    );
    assert_eq!(
        parse(&[("PRE_COMMIT", "1"), ("PRE_COMMIT_REMOTE_NAME", "origin"),])
            .expect("new root branch"),
        PreCommitPush::AllFiles
    );
}

/// Criterion 4: paths mode reads a real `.py` file and yields one whole-file
/// hunk covering every line.
///
/// "Every line" is the load-bearing half. A whole-file hunk that drops the
/// last line (or splits the file into per-line hunks) would change what the
/// LLM sees and where its findings land. Asserting the line count of the
/// hunk is what makes the whole-file contract testable.
#[tokio::test]
async fn paths_mode_yields_one_whole_file_hunk_covering_every_line() {
    let dir = tempfile::tempdir().expect("tempdir");
    let body = "a = 1\nb = 2\nc = 3\nd = 4\ne = 5\n";
    std::fs::write(dir.path().join("lib.py"), body).expect("write lib.py");

    let args = paths_args(vec![dir.path().join("lib.py")]);
    let work = resolve(&args, dir.path()).await.expect("paths resolve");

    assert!(
        work.read_failures.is_empty(),
        "a readable .py file must not land in read_failures, got {:?}",
        work.read_failures
    );
    assert_eq!(
        work.by_file.len(),
        1,
        "one file must yield one entry in by_file"
    );
    let hunks = &work.by_file[0];
    assert_eq!(
        hunks.len(),
        1,
        "whole-file mode must produce exactly one hunk per file"
    );
    let hunk = &hunks[0];
    assert_eq!(hunk.file_path, dir.path().join("lib.py"));
    assert_eq!(
        hunk.lines.len(),
        5,
        "every line of the file must appear in the hunk (got {})",
        hunk.lines.len()
    );
    assert_eq!(hunk.new_start, 1, "whole-file hunk starts at line 1");
    assert_eq!(
        hunk.new_count, 5,
        "whole-file hunk declares a line count matching its content"
    );
}

/// Criterion 5: a file larger than `READ_MAX_BYTES` lands in `read_failures`
/// as `FileTooLarge`, contributes no hunks, and **is still linted**.
///
/// Three assertions because each alone admits a wrong implementation: the
/// failure alone also holds for something that skipped the file and separately
/// recorded a reason; the `by_file` absence alone also holds for something
/// that skipped silently. The `lint_only` half is the one that was missing —
/// dropping the path entirely meant an LLM size limit silently switched ruff
/// off for that file, while the same file under `--staged` was still linted.
#[tokio::test]
async fn oversized_file_fails_is_not_in_by_file_and_is_still_linted() {
    let dir = tempfile::tempdir().expect("tempdir");
    // One byte over. The check is `bytes > limit`, so one over is the smallest
    // observable boundary - a regression to `>=` shows up here.
    let size = usize::try_from(READ_MAX_BYTES + 1).expect("read limit fits in usize");
    std::fs::write(dir.path().join("big.py"), vec![b'x'; size]).expect("write big.py");

    let args = paths_args(vec![dir.path().join("big.py")]);
    let work = resolve(&args, dir.path()).await.expect("paths resolve");

    assert!(
        work.by_file.is_empty(),
        "an oversize file must not enter by_file, got {} entries",
        work.by_file.len()
    );
    assert_eq!(
        work.lint_only,
        vec![dir.path().join("big.py")],
        "too large for the model is not too large for ruff; the path must \
         still reach the deterministic layer"
    );
    let reason = work
        .read_failures
        .get(&dir.path().join("big.py"))
        .expect("oversize file must appear in read_failures");
    match reason {
        FailureReason::FileTooLarge { bytes, limit } => {
            assert_eq!(*bytes, u64::try_from(size).expect("file size fits in u64"));
            assert_eq!(*limit, READ_MAX_BYTES);
        }
        other => panic!("expected FileTooLarge, got {other:?}"),
    }
}

/// Criterion 6: a file whose bytes are not valid UTF-8 lands in
/// `read_failures` as `Unreadable`.
///
/// The implementation reads with `std::fs::read_to_string`, which fails on
/// any byte that is not valid UTF-8. A binary blob slipped into the file
/// list is the realistic case (a config globbed in by mistake, a symlink to
/// `/dev/urandom`); the gate must not report it as analyzed.
#[tokio::test]
async fn non_utf8_file_lands_in_failures_as_unreadable() {
    let dir = tempfile::tempdir().expect("tempdir");
    let bytes = [0xFFu8, 0xFE, 0xFD, 0x80, 0x81];
    std::fs::write(dir.path().join("bad.py"), bytes).expect("write bad.py");

    let args = paths_args(vec![dir.path().join("bad.py")]);
    let work = resolve(&args, dir.path()).await.expect("paths resolve");

    assert!(
        work.by_file.is_empty(),
        "a non-UTF-8 file must not enter by_file"
    );
    let reason = work
        .read_failures
        .get(&dir.path().join("bad.py"))
        .expect("non-UTF-8 file must appear in read_failures");
    assert!(
        matches!(reason, FailureReason::Unreadable(_)),
        "expected Unreadable, got {reason:?}"
    );
}

/// Criterion 7: `--diff <ref>` against a ref that does not exist returns
/// `Err`, not an empty clean run.
///
/// A bare `TempDir` would also yield `Err` (git refuses because the
/// directory is not a repository), but for a different reason. To pin
/// "ref does not exist" specifically we init a repo with one commit so
/// the resolver reaches the actual ref lookup.
#[tokio::test]
async fn diff_against_nonexistent_ref_returns_err() {
    let dir = tempfile::tempdir().expect("tempdir");
    init_repo_with_commit(dir.path());

    let args = diff_args("definitely-not-a-real-ref-xyz");
    let result = resolve(&args, dir.path()).await;
    assert!(
        result.is_err(),
        "an unknown ref must return Err, got Ok({:?})",
        result.map(|w| (w.by_file.len(), w.read_failures.len()))
    );
}

/// Criterion 8: `--diff --something` is rejected before git runs. The error
/// must mention the ref, so the test cannot pass by failing for an
/// unrelated reason (git absent, repo missing, etc.).
#[tokio::test]
async fn diff_ref_starting_with_dash_is_rejected_before_git() {
    let dir = tempfile::tempdir().expect("tempdir");
    // Intentionally no `init_repo_with_commit` here: the guard fires before
    // git is ever invoked. If the guard regressed, the test would then
    // fail for a *different* reason (no git repo) that does not name the ref;
    // the message assertion below is what detects that regression.

    let args = diff_args("--output=/tmp/whatever");
    let result = resolve(&args, dir.path()).await;
    let err = match result {
        Ok(_) => panic!("a dash-prefixed ref must be rejected, got Ok(_)"),
        Err(e) => e,
    };
    let msg = format!("{err:#}");
    assert!(
        msg.contains("--output=/tmp/whatever"),
        "error must name the rejected ref, got {msg:?}"
    );
}

/// `git init` + one commit, so `--diff <something>` reaches the ref
/// resolution step instead of failing on "not a git repository".
///
/// Runs git via the process API. Failure to spawn git fails the test; a
/// clean exit with the expected file on disk is what subsequent assertions
/// rest on.
fn init_repo_with_commit(root: &std::path::Path) {
    crate::test_support::git_init(root);
    std::fs::write(root.join("seed.txt"), "seed\n").expect("seed");
    crate::test_support::git_add(root, "seed.txt");
    git_output(root, &["commit", "--quiet", "--no-verify", "-m", "init"]);
}

fn git_output(root: &std::path::Path, args: &[&str]) -> String {
    let output = crate::test_support::git(root)
        .args(args)
        .output()
        .expect("git spawns");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("git output is utf8")
        .trim()
        .to_owned()
}

fn git_commit(root: &std::path::Path, message: &str, paths: &[&str]) {
    for path in paths {
        crate::test_support::git_add(root, path);
    }
    git_output(root, &["commit", "--quiet", "--no-verify", "-m", message]);
}

/// A file of exactly `READ_MAX_BYTES` is accepted; one byte more is not.
///
/// Pins the boundary itself. `>` and `>=` both pass a test that only uses a
/// file far over the limit, so the comparison was free to drift by one.
#[tokio::test]
async fn a_file_exactly_at_the_size_limit_is_accepted_and_one_byte_over_is_not() {
    let dir = tempfile::tempdir().expect("tempdir");
    let limit = usize::try_from(READ_MAX_BYTES).expect("read limit fits in usize");

    let exact = dir.path().join("exact.py");
    std::fs::write(&exact, "a".repeat(limit)).expect("write exact");
    let over = dir.path().join("over.py");
    std::fs::write(&over, "a".repeat(limit + 1)).expect("write over");

    let args = paths_args(vec![exact.clone(), over.clone()]);
    let work = crate::cli::check::input::resolve(&args, dir.path())
        .await
        .expect("resolve");

    assert!(
        !work.read_failures.contains_key(&exact),
        "a file exactly at the limit must be accepted, failures: {:?}",
        work.read_failures.keys().collect::<Vec<_>>()
    );
    assert!(
        work.read_failures.contains_key(&over),
        "one byte over the limit must fail, failures: {:?}",
        work.read_failures.keys().collect::<Vec<_>>()
    );
}

/// Bare `drep check` — no paths, no `--staged`, no `--diff` — analyzes the
/// files under `root`.
///
/// The empty-paths case used to return the root path *unexpanded*, i.e. a
/// directory. `metadata` succeeded on it, the size gate passed, and
/// `read_to_string` then failed with "Is a directory" - so the plainest
/// invocation of the primary command reported the repo root as unreadable and
/// exited 2 having analyzed nothing. Nothing covered it: `src/cli/mod.rs`'s
/// parse tests even dropped `["drep", "check"]` from their list.
#[tokio::test]
async fn bare_check_with_no_paths_expands_the_root_instead_of_reading_a_directory() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("one.py"), "a = 1\n").expect("one.py");
    std::fs::create_dir_all(dir.path().join("pkg")).expect("mkdir");
    std::fs::write(dir.path().join("pkg/two.py"), "b = 2\n").expect("two.py");

    let args = paths_args(Vec::new());
    let work = resolve(&args, dir.path()).await.expect("bare resolve");

    assert!(
        work.read_failures.is_empty(),
        "the root directory must be walked, not read as a file, got {:?}",
        work.read_failures
    );
    assert_eq!(
        work.by_file.len(),
        2,
        "both .py files under root must be analyzed, got {:?}",
        work.by_file
            .iter()
            .filter_map(|h| h.first().map(|h| h.file_path.clone()))
            .collect::<Vec<_>>()
    );
}

/// A path the user named explicitly that does not exist is a failure, not a
/// clean run.
///
/// `files::expand_paths` silently skips missing paths - a contract inherited
/// from 1.x's `scan`, where a stale argument was not worth failing over. Behind
/// a gate whose whole thesis is that unanalyzed is never clean, it is:
/// `drep check typo.rs` would otherwise resolve to zero targets and print
/// "No issues found."
#[tokio::test]
async fn an_explicitly_named_missing_path_is_a_failure_not_a_clean_run() {
    let dir = tempfile::tempdir().expect("tempdir");
    let missing = dir.path().join("typo.py");

    let args = paths_args(vec![missing.clone()]);
    let work = resolve(&args, dir.path()).await.expect("resolve");

    assert!(
        work.by_file.is_empty(),
        "nothing to analyze, got {:?}",
        work.by_file.len()
    );
    assert!(
        work.read_failures.contains_key(&missing),
        "a named path that does not exist must reach the gate as a failure, got {:?}",
        work.read_failures
    );
}

/// A failure reason reads as one sentence, not two stacked prefixes.
///
/// The constructor embedded "could not read: {err}" while `one_line()` adds
/// "file could not be read: ", producing
/// `file could not be read: could not read: Is a directory (os error 21)`.
/// The reason owns the sentence; the constructor passes the raw error.
#[tokio::test]
async fn an_unreadable_file_reports_one_prefix_not_two() {
    let dir = tempfile::tempdir().expect("tempdir");
    let bad = dir.path().join("bad.py");
    std::fs::write(&bad, [0xFFu8, 0xFE, 0xFD]).expect("write bad bytes");

    let args = paths_args(vec![bad.clone()]);
    let work = resolve(&args, dir.path()).await.expect("resolve");

    let reason = work.read_failures.get(&bad).expect("bad.py must fail");
    let line = reason.to_string();
    assert_eq!(
        line.matches("could not").count(),
        1,
        "the reason must read as one sentence, got: {line}"
    );
}
