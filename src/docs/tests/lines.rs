//! The five per-line checks: boundaries, columns, and what must not fire.

use crate::docs::tests::{fires_once_at, of_kind, positions, silent, wide};
use crate::docs::{Check, LONG_LINE_MAX};

#[test]
fn long_line_boundary_is_exclusive() {
    // Both halves. A `>=` mutant passes the "121 fires" half on its own.
    silent(&wide(LONG_LINE_MAX), Check::LongLine);
    fires_once_at(
        &wide(LONG_LINE_MAX + 1),
        Check::LongLine,
        1,
        LONG_LINE_MAX as u32 + 1,
    );
}

#[test]
fn long_line_counts_characters_not_bytes() {
    // 120 em dashes is 360 bytes and 120 characters. A byte-length check would
    // report it, and would report every wide-character line in every
    // translated document.
    let content = "—".repeat(LONG_LINE_MAX);
    assert_eq!(content.len(), LONG_LINE_MAX * 3);
    silent(&content, Check::LongLine);
    assert_eq!(
        of_kind(&"—".repeat(LONG_LINE_MAX + 1), Check::LongLine).len(),
        1
    );
}

#[test]
fn long_line_message_names_the_actual_length() {
    let found = of_kind(&wide(150), Check::LongLine);
    assert!(found[0].message.contains("150"), "{}", found[0].message);
    assert!(
        found[0].message.contains(&LONG_LINE_MAX.to_string()),
        "{}",
        found[0].message
    );
}

#[test]
fn trailing_whitespace_column_points_at_the_first_stray_character() {
    // "abc  " -> the run starts at column 4, not at column 1 and not at the
    // end of the line.
    fires_once_at("abc  \n", Check::TrailingWhitespace, 1, 4);
    fires_once_at("abc\t\n", Check::TrailingWhitespace, 1, 4);
    fires_once_at("  \n", Check::TrailingWhitespace, 1, 1);
}

#[test]
fn trailing_whitespace_ignores_a_clean_line_and_interior_spaces() {
    silent("abc\n", Check::TrailingWhitespace);
    silent("a b c\n", Check::TrailingWhitespace);
    silent("", Check::TrailingWhitespace);
}

#[test]
fn trailing_whitespace_is_spaces_and_tabs_only() {
    // A trailing non-breaking space is a character the author typed; deleting
    // it silently is not this check's call, and reporting it as "trailing
    // whitespace" would send someone hunting for a space that is not there.
    silent("abc\u{a0}", Check::TrailingWhitespace);
}

#[test]
fn trailing_whitespace_counts_the_run() {
    let found = of_kind("abc   ", Check::TrailingWhitespace);
    assert!(found[0].message.contains('3'), "{}", found[0].message);
}

#[test]
fn tab_column_is_the_first_tab_not_the_last() {
    fires_once_at("a\tb\tc", Check::TabCharacter, 1, 2);
    fires_once_at("\ta", Check::TabCharacter, 1, 1);
    silent("a    b", Check::TabCharacter);
}

#[test]
fn empty_heading_fires_for_every_level_and_for_trailing_space() {
    for level in 1..=6usize {
        let hashes = "#".repeat(level);
        fires_once_at(&hashes, Check::EmptyHeading, 1, 1);
        fires_once_at(&format!("{hashes}   "), Check::EmptyHeading, 1, 1);
    }
}

#[test]
fn empty_heading_is_silent_for_a_real_heading() {
    silent("# Title", Check::EmptyHeading);
    silent("###### Deep", Check::EmptyHeading);
}

#[test]
fn missing_space_column_is_one_past_the_markers() {
    fires_once_at("#Heading", Check::MissingSpaceAfterHeading, 1, 2);
    fires_once_at("###Heading", Check::MissingSpaceAfterHeading, 1, 4);
}

#[test]
fn a_well_formed_heading_never_reports_a_missing_space() {
    // The regex this replaced had to exclude `#` from the character class,
    // because backtracking let it match the second `#` of `## Heading` and
    // report a well-formed heading as malformed. The hand-written scan counts
    // *every* leading marker, so the character it tests can never be one.
    for level in 1..=6usize {
        let heading = format!("{} Heading", "#".repeat(level));
        silent(&heading, Check::MissingSpaceAfterHeading);
    }
}

#[test]
fn seven_markers_is_not_a_heading_at_all() {
    // Markdown caps ATX headings at six. `#######text` renders as paragraph
    // text, so neither heading check has anything to say about it.
    silent("#######text", Check::MissingSpaceAfterHeading);
    silent("#######", Check::EmptyHeading);
    // Six still is a heading, so the bound is tested from both sides.
    fires_once_at("######text", Check::MissingSpaceAfterHeading, 1, 7);
}

#[test]
fn the_two_heading_checks_are_mutually_exclusive() {
    // One line can only be one of "no text" and "no space before the text".
    for line in ["#", "##  ", "#x", "###x", "# x", "#######"] {
        let both = positions(line, Check::EmptyHeading).len()
            + positions(line, Check::MissingSpaceAfterHeading).len();
        assert!(both <= 1, "{line:?} fired both heading checks");
    }
}

#[test]
fn a_heading_marker_must_start_the_line() {
    // An indented `#` is not an ATX heading. Firing here would report every
    // Python comment in an indented code block that escaped the fence check.
    silent(" #Heading", Check::MissingSpaceAfterHeading);
    silent(" #", Check::EmptyHeading);
}

#[test]
fn a_tab_after_the_markers_is_not_a_missing_space() {
    // `#\ttext` renders as a heading: the tab is whitespace. The tab itself is
    // reported by its own check, which is the correct finding for that line.
    silent("#\ttext", Check::MissingSpaceAfterHeading);
    assert_eq!(of_kind("#\ttext", Check::TabCharacter).len(), 1);
}

#[test]
fn per_line_checks_report_the_right_line_number() {
    // Every per-line check on line 3 of a 4-line file, so an implementation
    // that reported line 1 (or the loop index) fails all five at once.
    let content = format!("a\nb\n#Heading\t{}  \nd\n", wide(130));
    for check in [
        Check::MissingSpaceAfterHeading,
        Check::TabCharacter,
        Check::LongLine,
        Check::TrailingWhitespace,
    ] {
        let lines: Vec<u32> = positions(&content, check).iter().map(|p| p.0).collect();
        assert_eq!(lines, vec![3], "{}", check.as_str());
    }
}
