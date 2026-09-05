//! `analyze` as a whole: the shape of what comes out.

use std::collections::BTreeSet;
use std::path::Path;

use crate::docs::tests::{run, wide};
use crate::docs::{Check, analyze};

/// A document that trips all ten checks at once.
///
/// Built rather than written inline so the assertions below can say *which*
/// check is missing rather than "expected 10, got 9".
fn kitchen_sink() -> String {
    let mut doc = String::new();
    doc.push_str("##\n"); // empty_heading
    doc.push_str("#Heading\n"); // missing_space_after_heading
    doc.push_str("trailing   \n"); // trailing_whitespace
    doc.push_str("has\ttab\n"); // tab_character
    doc.push_str(&format!("{}\n", wide(130))); // long_line
    doc.push_str("see https://example.com/x\n"); // bare_url
    doc.push_str("[broken](\n"); // link_syntax_invalid
    doc.push_str("\n\n\n"); // multiple_blank_lines
    doc.push_str("```rust\n"); // unclosed_code_fence
    doc.push_str("fn main() {}\n");
    doc.push('\n'); // trailing_blank_lines
    doc
}

#[test]
fn every_check_can_fire() {
    // A check that can never fire is dead code wearing a name. This is the
    // test that catches a check wired into the enum but not into `analyze`.
    let fired: BTreeSet<String> = run(&kitchen_sink()).into_iter().map(|f| f.kind).collect();
    let expected: BTreeSet<String> = Check::ALL.iter().map(|c| c.as_str().to_owned()).collect();
    assert_eq!(fired, expected);
}

#[test]
fn findings_are_sorted_by_position() {
    let findings = run(&kitchen_sink());
    let positions: Vec<(u32, Option<u32>)> = findings.iter().map(|f| (f.line, f.column)).collect();
    let mut sorted = positions.clone();
    sorted.sort_unstable();
    assert_eq!(positions, sorted, "output must read top to bottom");
}

#[test]
fn two_runs_over_the_same_content_are_identical() {
    // The order the checks happen to run in must not reach the output.
    assert_eq!(run(&kitchen_sink()), run(&kitchen_sink()));
}

#[test]
fn a_clean_document_produces_nothing() {
    let content = "# Title\n\nSome prose with a [link](https://example.com).\n\n\
                   ```rust\nfn main() {}\n```\n\nMore prose.\n";
    assert_eq!(run(content), Vec::new());
}

#[test]
fn empty_and_blank_documents_are_distinguished() {
    assert_eq!(run(""), Vec::new());
    let findings = run("\n");
    let positions: Vec<_> = findings
        .iter()
        .map(|finding| (finding.kind.as_str(), finding.line, finding.column))
        .collect();
    assert_eq!(positions, vec![("trailing_blank_lines", 1, Some(1))]);
}

#[test]
fn every_finding_carries_the_path_it_was_given() {
    let findings = analyze(Path::new("docs/guide.md"), &kitchen_sink());
    assert!(!findings.is_empty());
    for finding in findings {
        assert_eq!(finding.file_path, "docs/guide.md");
    }
}

#[test]
fn every_finding_carries_a_suggestion_and_a_severity_from_its_check() {
    // The kind/severity/suggestion triple is assembled in one place; this is
    // what pins that a check site cannot hand-build a finding whose severity
    // disagrees with its kind.
    for finding in run(&kitchen_sink()) {
        let check = Check::ALL
            .into_iter()
            .find(|c| c.as_str() == finding.kind)
            .expect("kind must be a declared check");
        assert_eq!(finding.severity, check.severity(), "{}", finding.kind);
        assert_eq!(
            finding.suggestion.as_deref(),
            Some(check.suggestion()),
            "{}",
            finding.kind
        );
    }
}

#[test]
fn a_crlf_document_does_not_report_the_carriage_return_as_whitespace() {
    // Every line of a CRLF file ends with a `\r`. Reporting it would make
    // drep useless on any repository with a Windows contributor.
    let findings = run("# Title\r\n\r\nprose\r\n");
    assert_eq!(findings, Vec::new(), "{findings:?}");
}

#[test]
fn a_line_number_is_never_zero() {
    // 1-based throughout, because that is what an editor shows.
    for finding in run(&kitchen_sink()) {
        assert!(finding.line >= 1, "{}", finding.kind);
        assert!(finding.column.is_some_and(|c| c >= 1), "{}", finding.kind);
    }
}
