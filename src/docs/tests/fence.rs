//! Fence awareness, as a table.
//!
//! The point of `_fence_mask` is that it is the *single* answer, so the test
//! that matters is not "the mask is computed correctly" but "each check agrees
//! with it". Every check is listed below in exactly one of three tables, and a
//! completeness test fails if a new check is added without picking one - which
//! is the moment the decision is cheap to make.
//!
//! Each fence-sensitive case is asserted twice over the *same* snippet: once
//! in prose, where it must fire, and once inside a fence, where it must not. A
//! check that never fires at all would pass the second half on its own, which
//! is why the pair is the unit.

use crate::docs::Check;
use crate::docs::tests::{fires_once_at, of_kind, run, silent, wide};

/// Snippets that must fire in prose and stay silent inside a fence.
fn fence_sensitive() -> Vec<(Check, String)> {
    vec![
        (Check::EmptyHeading, "##".to_owned()),
        // The specific false positive the mask was built for: a shebang at the
        // top of a `bash` sample read as a heading with no space after its `#`.
        (Check::MissingSpaceAfterHeading, "#!/bin/bash".to_owned()),
        (Check::LongLine, wide(121)),
        (Check::TabCharacter, "col\tcol".to_owned()),
        (Check::BareUrl, "see https://example.com/x".to_owned()),
        (Check::LinkSyntaxInvalid, "[text](url".to_owned()),
    ]
}

/// Snippets that must fire in prose *and* inside a fence.
///
/// One member, deliberately. Trailing whitespace is the only check whose
/// advice - "remove it" - is equally correct in prose and in a code sample.
fn fence_blind() -> Vec<(Check, String)> {
    vec![(Check::TrailingWhitespace, "text  ".to_owned())]
}

/// Checks that are not about a single line, so a one-line snippet cannot
/// express them. Each has its own test in `blocks.rs`.
const STRUCTURAL: [Check; 3] = [
    Check::MultipleBlankLines,
    Check::TrailingBlankLines,
    Check::UnclosedCodeFence,
];

#[test]
fn every_check_has_declared_a_fence_position() {
    let mut declared: Vec<&str> = fence_sensitive()
        .iter()
        .chain(fence_blind().iter())
        .map(|(c, _)| c.as_str())
        .chain(STRUCTURAL.iter().map(|c| c.as_str()))
        .collect();
    declared.sort_unstable();
    let mut all: Vec<&str> = Check::ALL.iter().map(|c| c.as_str()).collect();
    all.sort_unstable();
    assert_eq!(
        declared, all,
        "a check must appear in exactly one of fence_sensitive / fence_blind / \
         STRUCTURAL - if you added one, decide what it does inside a fence"
    );
}

#[test]
fn fence_sensitive_checks_fire_in_prose() {
    for (check, snippet) in fence_sensitive() {
        assert_eq!(
            of_kind(&snippet, check).len(),
            1,
            "{} must fire on {snippet:?} standing alone",
            check.as_str()
        );
    }
}

#[test]
fn fence_sensitive_checks_are_silent_inside_a_fence() {
    for (check, snippet) in fence_sensitive() {
        let fenced = format!("```text\n{snippet}\n```\n");
        silent(&fenced, check);
    }
}

#[test]
fn fence_blind_checks_fire_in_prose_and_inside_a_fence() {
    for (check, snippet) in fence_blind() {
        assert_eq!(of_kind(&snippet, check).len(), 1, "{}", check.as_str());
        let fenced = format!("```text\n{snippet}\n```\n");
        // Line 2: the snippet sits between the delimiters.
        fires_once_at(&fenced, check, 2, snippet.chars().count() as u32 - 1);
    }
}

#[test]
fn the_delimiter_line_is_itself_inside_the_fence() {
    // Not an off-by-one. A ```` ```javascript ```` opener padded out past the
    // limit is code punctuation, not a paragraph that needs wrapping. If the
    // mask marked only the lines *between* delimiters, this would fire.
    let opener = format!("```{}", wide(130));
    let content = format!("{opener}\ncode\n```\n");
    silent(&content, Check::LongLine);
    // Same content unfenced does fire, so the assertion above is about the
    // fence and not about the line being short.
    assert_eq!(of_kind(&opener.replace('`', "x"), Check::LongLine).len(), 1);
}

#[test]
fn an_indented_fence_still_opens_a_fence() {
    // A fence nested in a list item is indented. Reading it as prose would put
    // the whole sample back into every prose check.
    let content = "- item:\n\n  ```bash\n  #!/bin/bash\n  ```\n";
    silent(content, Check::MissingSpaceAfterHeading);
}

#[test]
fn text_after_a_closing_fence_is_prose_again() {
    // The mask must toggle, not latch. A latched mask would silence every
    // check for the rest of the file after the first code sample - and a file
    // whose checks all stop firing halfway down looks exactly like a clean
    // file.
    let content = "```bash\n#!/bin/bash\n```\n\n#Heading\n";
    fires_once_at(content, Check::MissingSpaceAfterHeading, 5, 2);
}

#[test]
fn a_file_with_no_fence_at_all_leaves_every_line_in_prose() {
    let content = "#Heading\n\nmore\n";
    assert!(run(content).iter().any(|f| f.line == 1));
}
