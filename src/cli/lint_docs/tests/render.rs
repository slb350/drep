//! The exact bytes `lint-docs` prints.
//!
//! Pinned rather than smoke-tested: this output is what a developer reads in a
//! pre-commit hook, and the report-only footer in particular is the only
//! signal that a run which printed forty findings deliberately exited 0.

use crate::Exit;
use crate::analysis::findings::{Finding, Severity};
use crate::analysis::result::FailureReason;
use crate::cli::lint_docs::{LintOutcome, render};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// A finding to render.
fn finding(kind: &str, line: u32, column: Option<u32>) -> Finding {
    Finding {
        kind: kind.to_owned(),
        severity: Severity::Info,
        file_path: "README.md".to_owned(),
        line,
        column,
        message: "something".to_owned(),
        suggestion: Some("do the other thing".to_owned()),
    }
}

/// Render an outcome and hand back the bytes as a string.
fn rendered(outcome: &LintOutcome, strict: bool) -> String {
    let mut buffer = Vec::new();
    render::render_to(&mut buffer, outcome, strict).expect("render");
    String::from_utf8(buffer).expect("utf8")
}

/// An outcome carrying `findings` and `failures`.
fn outcome(findings: Vec<Finding>, failures: Vec<(&str, FailureReason)>) -> LintOutcome {
    let failures: BTreeMap<PathBuf, FailureReason> = failures
        .into_iter()
        .map(|(path, reason)| (PathBuf::from(path), reason))
        .collect();
    let exit = if !failures.is_empty() {
        Exit::Unanalyzed
    } else if findings.is_empty() {
        Exit::Clean
    } else {
        Exit::FoundIssues
    };
    LintOutcome {
        findings,
        failures,
        exit,
    }
}

#[test]
fn a_clean_run_prints_exactly_the_clean_line() {
    let text = rendered(&outcome(vec![], vec![]), false);
    assert_eq!(text, "No issues found.\n");
}

#[test]
fn a_finding_renders_position_severity_kind_and_message() {
    let text = rendered(
        &outcome(vec![finding("long_line", 12, Some(121))], vec![]),
        false,
    );
    assert!(
        text.starts_with("README.md:12:121: info [long_line] something\n"),
        "{text}"
    );
}

#[test]
fn the_suggestion_follows_its_own_finding() {
    // Two findings. Printing every finding and then every suggestion detaches
    // them, and the first suggestion reads as if it belonged to the second
    // finding - the bug `check`'s renderer had.
    let text = rendered(
        &outcome(
            vec![finding("a", 1, Some(1)), finding("b", 2, Some(1))],
            vec![],
        ),
        false,
    );
    let lines: Vec<&str> = text.lines().collect();
    assert!(lines[0].contains("[a]"), "{text}");
    assert_eq!(lines[1], "    suggestion: do the other thing");
    assert!(lines[2].contains("[b]"), "{text}");
    assert_eq!(lines[3], "    suggestion: do the other thing");
}

#[test]
fn a_finding_with_no_column_omits_the_colon() {
    let text = rendered(&outcome(vec![finding("k", 7, None)], vec![]), false);
    assert!(
        text.starts_with("README.md:7: info [k] something\n"),
        "{text}"
    );
}

#[test]
fn the_footer_says_report_only_unless_strict() {
    let one = outcome(vec![finding("k", 1, Some(1))], vec![]);
    let lenient = rendered(&one, false);
    assert!(
        lenient.ends_with("1 issue(s) found (report only; pass --strict to fail).\n"),
        "{lenient}"
    );
    let strict = rendered(&one, true);
    assert!(strict.ends_with("1 issue(s) found.\n"), "{strict}");
    assert!(!strict.contains("report only"), "{strict}");
}

#[test]
fn failures_are_listed_with_their_reason() {
    let text = rendered(
        &outcome(
            vec![],
            vec![(
                "notes.txt",
                FailureReason::Unsupported {
                    extension: Some(".txt".to_owned()),
                    hint: None,
                },
            )],
        ),
        false,
    );
    assert_eq!(
        text,
        "1 file(s) could not be analyzed:\n  notes.txt: no analyzer for `.txt` files\n"
    );
    // No clean line, and no findings footer: neither applies to a run that
    // produced no findings but did not complete.
    assert!(!text.contains("No issues found"), "{text}");
    assert!(!text.contains("issue(s) found"), "{text}");
}

#[test]
fn a_blank_line_separates_the_two_blocks_only_when_both_are_present() {
    let both = rendered(
        &outcome(
            vec![finding("k", 1, Some(1))],
            vec![("x.txt", FailureReason::Unreadable("gone".to_owned()))],
        ),
        false,
    );
    assert!(
        both.contains("something\n    suggestion: do the other thing\n\n1 file(s)"),
        "{both}"
    );

    // Failures alone must not open with a stray empty line.
    let failures_only = rendered(
        &outcome(
            vec![],
            vec![("x.txt", FailureReason::Unreadable("gone".to_owned()))],
        ),
        false,
    );
    assert!(failures_only.starts_with("1 file(s)"), "{failures_only:?}");
}
