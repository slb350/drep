//! Which files the command reads, and what it does with a path it will not.
//!
//! The distinction under test is the one `check` already drew for a
//! non-existent argument: a **walk** that finds nothing analyzable is
//! legitimately empty, an **explicitly named** path that goes unanalyzed is
//! not. Getting this wrong in the permissive direction is how a file drep
//! declined to look at gets reported as clean.

use crate::Exit;
use crate::analysis::result::FailureReason;
use crate::cli::lint_docs::tests::{repo, run_in, walk};

#[test]
fn a_bare_invocation_walks_the_tree_for_markdown() {
    let dir = repo(&[
        ("README.md", "#Heading\n"),
        ("docs/guide.md", "#Other\n"),
        ("src/main.rs", "fn main() {}\n"),
    ]);
    let outcome = walk(dir.path());
    assert_eq!(outcome.findings.len(), 2, "{:?}", outcome.findings);
    assert!(outcome.failures.is_empty());
    // The `.rs` file was walked past, not failed: a walk has no opinion about
    // a file type it does not own.
    assert_eq!(outcome.exit, Exit::Clean);
}

#[test]
fn a_directory_holding_no_markdown_is_a_legitimately_empty_walk() {
    let dir = repo(&[("src/main.rs", "fn main() {}\n")]);
    let outcome = run_in(dir.path(), &["src"], true);
    assert!(outcome.findings.is_empty());
    assert!(outcome.failures.is_empty(), "{:?}", outcome.failures);
    assert_eq!(outcome.exit, Exit::Clean);
}

#[test]
fn a_named_non_markdown_file_is_a_failure_not_a_skip() {
    // The banned move, in the direction `lint-docs` could make it: accept a
    // path, analyze nothing, report clean.
    let dir = repo(&[("src/main.rs", "fn main() {}\n")]);
    let outcome = run_in(dir.path(), &["src/main.rs"], false);
    assert_eq!(outcome.exit, Exit::Unanalyzed);
    let reason = outcome
        .failures
        .values()
        .next()
        .expect("the named file must be reported");
    assert!(
        matches!(reason, FailureReason::Unsupported { .. }),
        "{reason:?}"
    );
}

#[test]
fn a_named_source_file_is_redirected_to_check() {
    let dir = repo(&[("main.rs", "fn main() {}\n")]);
    let outcome = run_in(dir.path(), &["main.rs"], false);
    let reason = outcome.failures.values().next().expect("reported");
    assert_eq!(
        reason,
        &FailureReason::Unsupported {
            extension: Some(".rs".to_owned()),
            hint: Some("run `drep check` instead".to_owned()),
        }
    );
    assert!(reason.one_line().contains("drep check"), "{reason:?}");
}

#[test]
fn a_named_file_no_command_handles_gets_no_hint() {
    // drep genuinely has nothing to say about a `.png`. Inventing a
    // suggestion would be worse than admitting that.
    let dir = repo(&[("logo.png", "")]);
    let outcome = run_in(dir.path(), &["logo.png"], false);
    let reason = outcome.failures.values().next().expect("reported");
    assert_eq!(
        reason,
        &FailureReason::Unsupported {
            extension: Some(".png".to_owned()),
            hint: None,
        }
    );
}

#[test]
fn a_named_file_with_no_extension_reads_as_such() {
    let dir = repo(&[("Makefile", "all:\n")]);
    let outcome = run_in(dir.path(), &["Makefile"], false);
    let reason = outcome.failures.values().next().expect("reported");
    assert_eq!(
        reason,
        &FailureReason::Unsupported {
            extension: None,
            hint: None,
        }
    );
    // The message must still be a sentence, not "no analyzer for `` files".
    assert_eq!(reason.one_line(), "no analyzer for files with no extension");
}

#[test]
fn a_named_path_that_does_not_exist_is_a_failure() {
    let dir = repo(&[("README.md", "# T\n")]);
    let outcome = run_in(dir.path(), &["missing.md"], false);
    assert_eq!(outcome.exit, Exit::Unanalyzed);
    assert!(
        matches!(
            outcome.failures.values().next(),
            Some(FailureReason::Unreadable(_))
        ),
        "{:?}",
        outcome.failures
    );
}

#[test]
fn a_named_markdown_file_is_analyzed_and_others_beside_it_are_not() {
    let dir = repo(&[("a.md", "#A\n"), ("b.md", "#B\n")]);
    let outcome = run_in(dir.path(), &["a.md"], false);
    assert_eq!(outcome.findings.len(), 1);
    assert!(outcome.findings[0].file_path.ends_with("a.md"));
}

#[test]
fn a_file_that_is_not_utf8_is_a_failure_rather_than_being_skipped() {
    let dir = repo(&[]);
    std::fs::write(dir.path().join("bad.md"), [0xff, 0xfe, 0x00]).expect("write");
    let outcome = walk(dir.path());
    assert_eq!(outcome.exit, Exit::Unanalyzed);
    assert!(
        matches!(
            outcome.failures.values().next(),
            Some(FailureReason::Unreadable(_))
        ),
        "{:?}",
        outcome.failures
    );
}

#[test]
fn findings_are_ordered_by_file_then_position() {
    let dir = repo(&[("b.md", "#B\n\n\n\n"), ("a.md", "#A\n")]);
    let outcome = walk(dir.path());
    let keys: Vec<(String, u32)> = outcome
        .findings
        .iter()
        .map(|f| (f.file_path.clone(), f.line))
        .collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted, "{keys:?}");
}
