//! The exit-code contract, which is the whole reason this command exists in a
//! hook.
//!
//! Two rules that must stay independent. `--strict` governs *findings*; a file
//! that could not be read is not a finding, it is the absence of analysis, and
//! no flag turns that into a clean run.

use crate::Exit;
use crate::analysis::findings::Severity;
use crate::cli::lint_docs::tests::{repo, run_failing_on, run_in};

#[test]
fn a_clean_tree_exits_clean_under_both_modes() {
    let dir = repo(&[("README.md", "# Title\n\nprose\n")]);
    assert_eq!(run_in(dir.path(), &[], false).exit, Exit::Clean);
    assert_eq!(run_in(dir.path(), &[], true).exit, Exit::Clean);
}

#[test]
fn findings_are_report_only_by_default_and_block_under_strict() {
    // Both halves over the *same* tree. A gate that always exits 0 passes the
    // first assertion; one that always exits 1 passes the second.
    let dir = repo(&[("README.md", "#Heading\n")]);
    let lenient = run_in(dir.path(), &[], false);
    assert!(!lenient.findings.is_empty());
    assert_eq!(lenient.exit, Exit::Clean);

    let strict = run_in(dir.path(), &[], true);
    assert_eq!(
        strict.findings, lenient.findings,
        "same findings, both ways"
    );
    assert_eq!(strict.exit, Exit::FoundIssues);
}

#[test]
fn an_unanalyzable_file_exits_two_even_report_only() {
    // The rule `--strict` does *not* govern. A hook running report-only still
    // has to hear that drep never read the file.
    let dir = repo(&[("README.md", "# Title\n")]);
    let outcome = run_in(dir.path(), &["missing.md"], false);
    assert!(outcome.findings.is_empty());
    assert_eq!(outcome.exit, Exit::Unanalyzed);
}

#[test]
fn a_failure_outranks_a_finding() {
    // Both present at once. Exit 1 would tell a user "there are issues" for a
    // run that also did not look at a file they named - and they would fix the
    // issues and never learn about the file.
    let dir = repo(&[("README.md", "#Heading\n")]);
    let outcome = run_in(dir.path(), &["README.md", "missing.md"], true);
    assert!(!outcome.findings.is_empty());
    assert!(!outcome.failures.is_empty());
    assert_eq!(outcome.exit, Exit::Unanalyzed);
}

#[test]
fn an_empty_repository_is_clean_not_unanalyzed() {
    // No markdown anywhere, nothing named. There is nothing to report and
    // nothing was skipped.
    let dir = repo(&[]);
    let outcome = run_in(dir.path(), &[], true);
    assert_eq!(outcome.exit, Exit::Clean);
    assert!(outcome.failures.is_empty());
}

/// `--fail-on error` is the calibration the published hook ships.
///
/// The doc checks are severity-ranked precisely so a hook can block on the one
/// defect that changes how the rest of the file renders and stay quiet about
/// line length. `--strict` cannot express that: it blocks on anything, which
/// over this repository is 75 findings and not one of them above `info`.
#[test]
fn fail_on_error_ignores_findings_below_the_threshold() {
    // Trailing whitespace and a long line: `info`, both of them.
    let dir = repo(&[("README.md", "# Title\n\nprose   \n")]);
    let outcome = run_failing_on(dir.path(), &[], Severity::Error);
    assert!(
        !outcome.findings.is_empty(),
        "the findings must still be reported"
    );
    assert!(
        outcome
            .findings
            .iter()
            .all(|f| f.severity < Severity::Error),
        "fixture must produce only sub-threshold findings, got {:?}",
        outcome.findings
    );
    assert_eq!(outcome.exit, Exit::Clean);
}

#[test]
fn fail_on_error_blocks_on_an_unclosed_fence() {
    // The one `error` among the ten checks: everything below the delimiter
    // renders as code.
    let dir = repo(&[("README.md", "# Title\n\n```rust\nlet x = 1;\n")]);
    let outcome = run_failing_on(dir.path(), &[], Severity::Error);
    assert!(
        outcome
            .findings
            .iter()
            .any(|f| f.severity == Severity::Error),
        "fixture must produce an error-severity finding, got {:?}",
        outcome.findings
    );
    assert_eq!(outcome.exit, Exit::FoundIssues);
}

/// The shorthand and the threshold are one mechanism, not two.
#[test]
fn strict_gates_exactly_as_fail_on_info_does() {
    let dir = repo(&[("README.md", "#Heading\n\nprose   \n")]);
    let strict = run_in(dir.path(), &[], true);
    let explicit = run_failing_on(dir.path(), &[], Severity::Info);
    assert_eq!(strict.findings, explicit.findings);
    assert_eq!(strict.exit, explicit.exit);
    assert_eq!(strict.exit, Exit::FoundIssues);
}

/// A threshold governs findings, never the absence of analysis.
#[test]
fn fail_on_error_still_exits_two_for_a_file_it_could_not_read() {
    let dir = repo(&[("README.md", "# Title\n")]);
    let outcome = run_failing_on(dir.path(), &["missing.md"], Severity::Error);
    assert!(outcome.findings.is_empty());
    assert_eq!(outcome.exit, Exit::Unanalyzed);
}
