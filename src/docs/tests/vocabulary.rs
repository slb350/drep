//! [`Check`] itself: the wire names, the severity rule, the suggestions.
//!
//! These are the user-visible contract. A user who scripted against 1.x's
//! `type=` strings must keep working, and `--strict` gates on the severity, so
//! a variant silently changing tier changes which commits are blocked.

use std::collections::BTreeSet;

use crate::analysis::findings::Severity;
use crate::docs::Check;

/// The exact ten strings the 1.x Python analyzer emitted.
///
/// Written out rather than derived: this is the compatibility contract, so the
/// test has to hold a second, independent copy of it. Deriving it from
/// `Check::as_str` would assert only that the function equals itself.
const PYTHON_TYPES: [&str; 10] = [
    "bare_url",
    "empty_heading",
    "link_syntax_invalid",
    "long_line",
    "missing_space_after_heading",
    "multiple_blank_lines",
    "tab_character",
    "trailing_blank_lines",
    "trailing_whitespace",
    "unclosed_code_fence",
];

#[test]
fn wire_names_are_exactly_the_ten_python_emitted() {
    let ours: BTreeSet<&str> = Check::ALL.iter().map(|c| c.as_str()).collect();
    let theirs: BTreeSet<&str> = PYTHON_TYPES.into_iter().collect();
    assert_eq!(ours, theirs);
}

#[test]
fn all_lists_every_variant_exactly_once() {
    // `ALL` is what the rest of the suite iterates, so a variant missing from
    // it would make every other test in this module silently narrower.
    let unique: BTreeSet<Check> = Check::ALL.into_iter().collect();
    assert_eq!(unique.len(), Check::ALL.len());
    assert_eq!(unique.len(), PYTHON_TYPES.len());
}

#[test]
fn severity_follows_the_does_it_change_rendering_rule() {
    // Pinned one by one. A single `assert!(matches!(..))` over the whole set
    // would pass with every check collapsed onto one tier, which is precisely
    // the failure that makes `--strict` useless.
    let expected = [
        (Check::UnclosedCodeFence, Severity::Error),
        (Check::EmptyHeading, Severity::Warning),
        (Check::MissingSpaceAfterHeading, Severity::Warning),
        (Check::LinkSyntaxInvalid, Severity::Warning),
        (Check::BareUrl, Severity::Info),
        (Check::LongLine, Severity::Info),
        (Check::MultipleBlankLines, Severity::Info),
        (Check::TabCharacter, Severity::Info),
        (Check::TrailingBlankLines, Severity::Info),
        (Check::TrailingWhitespace, Severity::Info),
    ];
    assert_eq!(
        expected.len(),
        Check::ALL.len(),
        "a check has no expectation"
    );
    for (check, severity) in expected {
        assert_eq!(check.severity(), severity, "{}", check.as_str());
    }
}

#[test]
fn exactly_one_check_blocks_at_error() {
    // The rule says only a defect that changes how the *rest of the file*
    // renders earns `Error`. If a second check ever reaches that tier it is a
    // deliberate decision, and this test is where it gets made.
    let errors: Vec<&str> = Check::ALL
        .iter()
        .filter(|c| c.severity() == Severity::Error)
        .map(|c| c.as_str())
        .collect();
    assert_eq!(errors, vec!["unclosed_code_fence"]);
}

#[test]
fn every_check_carries_a_distinct_non_empty_suggestion() {
    let suggestions: BTreeSet<&str> = Check::ALL.iter().map(|c| c.suggestion()).collect();
    assert_eq!(
        suggestions.len(),
        Check::ALL.len(),
        "two checks share a suggestion, so one of them is telling the user \
         to fix the wrong thing"
    );
    for check in Check::ALL {
        assert!(!check.suggestion().is_empty(), "{}", check.as_str());
    }
}
