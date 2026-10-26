//! The deterministic layer in branch mode (`--diff`, `--tip`, and the
//! published pre-commit pre-push hook's range): the semantic layer reviews the
//! tip commit's diff while the linters read the working tree, so a linter that
//! would read working-tree content the tip does not hold is refused rather than
//! run, exactly as staged mode refuses one for the committing index.

use std::path::{Path, PathBuf};

use super::deterministic_staged::ruff_fixture;
use crate::analysis::result::FailureReason;
use crate::cli::OutputFormat;
use crate::cli::check::input::{PreCommitPush, resolve, resolve_pre_commit};
use crate::cli::check::{CheckArgs, deterministic};
use crate::test_support::{git_commit_all, git_init, git_output};

/// `--diff <base>`, with `--tip <tip>` when one is given.
fn branch_args(base: &str, tip: Option<&str>) -> CheckArgs {
    CheckArgs {
        paths: Vec::new(),
        staged: false,
        diff: Some(base.to_owned()),
        tip: tip.map(str::to_owned),
        pre_commit_push: false,
        format: OutputFormat::Text,
        fail_on: None,
        cache_only: false,
        push_gate: false,
        max_review_rounds: None,
        unlimited_reviews: false,
    }
}

/// A repository whose `main` holds a clean `a.py` and a configured ruff stub,
/// and whose `feature` branch changes `a.py`. The stub and its record are
/// ignored, so they are not themselves working-tree content the tip lacks.
///
/// Returns the stub's argv record and the base commit.
fn branch_fixture(root: &Path) -> (PathBuf, String) {
    git_init(root);
    let argv = ruff_fixture(root);
    std::fs::write(root.join(".gitignore"), "venv/\nargv.txt\n").expect(".gitignore");
    std::fs::write(root.join("a.py"), "a = 1\n").expect("a.py");
    git_commit_all(root, "base");
    let base = git_output(root, &["rev-parse", "HEAD"]);
    git_output(root, &["checkout", "--quiet", "-b", "feature"]);
    std::fs::write(root.join("a.py"), "a = 2\n").expect("feature a.py");
    git_commit_all(root, "feature");
    (argv, base)
}

fn assert_refused(failures: &std::collections::BTreeMap<PathBuf, FailureReason>, file: &Path) {
    match failures.get(file) {
        Some(FailureReason::UncommittedChanges { tool, paths }) => {
            assert_eq!(tool, "ruff");
            assert_eq!(paths, &[PathBuf::from("a.py")]);
        }
        other => panic!("expected UncommittedChanges for {file:?}, got {other:?}"),
    }
}

/// The pre-push hook passes the pushed ref's oid as `--tip`, and that ref need
/// not be checked out. Linting the working tree there checks `main`'s `a.py`
/// and reports the pushed one clean, so the tool must not run.
#[tokio::test]
async fn a_pushed_tip_that_is_not_checked_out_does_not_lint_the_working_tree() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (argv, base) = branch_fixture(dir.path());
    let tip = git_output(dir.path(), &["rev-parse", "feature"]);
    git_output(dir.path(), &["checkout", "--quiet", "main"]);

    let work = resolve(&branch_args(&base, Some(&tip)), dir.path(), false)
        .await
        .expect("branch diff resolves");
    let (findings, failures, _compiled) = deterministic::run(&work, dir.path()).await;

    assert!(
        !argv.exists(),
        "ruff must not lint a working tree that does not hold the pushed tip"
    );
    assert!(findings.is_empty(), "got {findings:?}");
    assert_refused(&failures, Path::new("a.py"));
}

/// The ordinary push of the checked-out branch from a dirty working tree: the
/// tip is HEAD, but an uncommitted edit means the linter would read content
/// the pushed commit does not contain.
#[tokio::test]
async fn an_uncommitted_edit_refuses_a_linter_in_branch_mode() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (argv, base) = branch_fixture(dir.path());
    std::fs::write(dir.path().join("a.py"), "a = 3\n").expect("dirty a.py");

    let work = resolve(&branch_args(&base, None), dir.path(), false)
        .await
        .expect("branch diff resolves");
    let (_findings, failures, _compiled) = deterministic::run(&work, dir.path()).await;

    assert!(!argv.exists(), "ruff must not lint an uncommitted edit");
    assert_refused(&failures, Path::new("a.py"));
}

/// The published pre-commit hook's range names its tip the same way.
#[tokio::test]
async fn a_pre_commit_push_range_does_not_lint_a_working_tree_without_its_tip() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (argv, base) = branch_fixture(dir.path());
    let tip = git_output(dir.path(), &["rev-parse", "feature"]);
    git_output(dir.path(), &["checkout", "--quiet", "main"]);

    let range = PreCommitPush::Range {
        from: base,
        to: tip,
    };
    let work = resolve_pre_commit(dir.path(), &range, false)
        .await
        .expect("pre-commit range resolves");
    let (_findings, failures, _compiled) = deterministic::run(&work, dir.path()).await;

    assert!(!argv.exists(), "ruff must not lint a tree without the tip");
    assert_refused(&failures, Path::new("a.py"));
}

/// A clean checkout of the pushed tip is the ordinary case and runs the tool
/// once: the refusal is about differing content, not about branch mode.
#[tokio::test]
async fn a_clean_checkout_of_the_tip_runs_the_linter() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (argv, base) = branch_fixture(dir.path());
    let tip = git_output(dir.path(), &["rev-parse", "HEAD"]);

    let work = resolve(&branch_args(&base, Some(&tip)), dir.path(), false)
        .await
        .expect("branch diff resolves");
    let (_findings, failures, _compiled) = deterministic::run(&work, dir.path()).await;

    assert!(failures.is_empty(), "got {failures:?}");
    let runs = std::fs::read_to_string(&argv).expect("ruff ran");
    assert_eq!(runs.lines().count(), 1, "ruff runs once: {runs}");
}
