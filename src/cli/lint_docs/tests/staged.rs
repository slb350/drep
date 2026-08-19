//! `lint-docs --staged`: the mode `drep init`'s hook uses.
//!
//! Wired in via this directory's `mod.rs` - a test file no `mod` points at is
//! silently not compiled.
//!
//! Running the command bare over a repository would also "work", and is what
//! the hook did before this mode existed: it reports findings in files the
//! commit never touched, and per-commit noise about someone else's document is
//! how a report-only gate gets switched off.

use std::fs;

use crate::Exit;
use crate::cli::lint_docs::LintDocsArgs;
use crate::test_support::git_init;

/// Arguments for a `--staged` run.
fn staged_args() -> LintDocsArgs {
    LintDocsArgs {
        paths: Vec::new(),
        staged: true,
        strict: false,
        fail_on: None,
    }
}

#[tokio::test]
async fn only_the_staged_markdown_is_analyzed() {
    let dir = tempfile::tempdir().expect("tempdir");
    git_init(dir.path());

    // One staged document with a finding, one unstaged with a worse one. A
    // command that walked the tree would report both.
    fs::write(dir.path().join("staged.md"), "#Heading\n").expect("write");
    fs::write(dir.path().join("untracked.md"), "# Title\n\n```rust\n").expect("write");
    add(dir.path(), "staged.md");

    let outcome = crate::cli::lint_docs::outcome_for(&staged_args(), dir.path())
        .await
        .expect("staged run");
    assert!(
        outcome
            .findings
            .iter()
            .all(|f| f.file_path.ends_with("staged.md")),
        "only the staged document may be analyzed, got {:?}",
        outcome.findings
    );
    assert!(!outcome.findings.is_empty());
}

/// Nothing staged is a clean run, not a walk of the repository.
///
/// `expand_named` resolves an empty argument list to `root` - that is what
/// makes bare `drep lint-docs` mean "this tree". Reusing it for `--staged`
/// would turn "no markdown in this commit" into "lint every document in the
/// repository", on every commit.
#[tokio::test]
async fn no_staged_markdown_is_clean_and_analyzes_nothing() {
    let dir = tempfile::tempdir().expect("tempdir");
    git_init(dir.path());
    fs::write(dir.path().join("untracked.md"), "#Heading\n").expect("write");

    let outcome = crate::cli::lint_docs::outcome_for(&staged_args(), dir.path())
        .await
        .expect("staged run");
    assert!(outcome.findings.is_empty(), "got {:?}", outcome.findings);
    assert!(outcome.failures.is_empty());
    assert_eq!(outcome.exit, Exit::Clean);
}

/// `--staged` gates on the same threshold every other mode does.
#[tokio::test]
async fn the_threshold_applies_in_staged_mode() {
    let dir = tempfile::tempdir().expect("tempdir");
    git_init(dir.path());
    fs::write(
        dir.path().join("doc.md"),
        "# Title\n\n```rust\nlet x = 1;\n",
    )
    .expect("write");
    add(dir.path(), "doc.md");

    let mut args = staged_args();
    args.fail_on = Some(crate::analysis::findings::Severity::Error);
    let outcome = crate::cli::lint_docs::outcome_for(&args, dir.path())
        .await
        .expect("staged run");
    assert_eq!(outcome.exit, Exit::FoundIssues);

    args.fail_on = None;
    let report_only = crate::cli::lint_docs::outcome_for(&args, dir.path())
        .await
        .expect("staged run");
    assert!(!report_only.findings.is_empty());
    assert_eq!(report_only.exit, Exit::Clean);
}

/// `git add`, through the crate helper that scrubs `GIT_DIR`, `GIT_WORK_TREE`
/// and `GIT_INDEX_FILE` from the environment.
///
/// A bare `Command::new("git")` works until the suite runs inside another git
/// invocation - drep's own pre-commit hook, or a mutation run under it - where
/// an inherited `GIT_INDEX_FILE` points this `add` at the outer repository's
/// index.
fn add(root: &std::path::Path, path: &str) {
    let output = crate::test_support::git(root)
        .args(["add", path])
        .output()
        .expect("git add must run");
    assert!(
        output.status.success(),
        "git add failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// The command refuses paths and `--staged` together: two answers to "what am
/// I linting" is one too many.
#[test]
fn staged_and_paths_are_mutually_exclusive() {
    use clap::Parser;
    assert!(crate::cli::Cli::try_parse_from(["drep", "lint-docs", "--staged", "a.md"]).is_err());
}
