//! `stream_detail`, which bounds a tool's own stream for the `detail` field.
//!
//! The bounding itself is `crate::text::excerpt` and is tested there. What
//! needs its own cases is the pair of decisions this wrapper makes on top of
//! it: that the stream is trimmed, and that an empty stream stays empty
//! rather than becoming `excerpt`'s `<nothing>` placeholder.

use crate::languages::runner::stream_detail;

#[test]
fn an_empty_stream_stays_empty() {
    // The reason the wrapper exists. `doctor` prints this field after a
    // colon on a clean run, where `<nothing>` reads as a fault.
    assert_eq!(stream_detail(""), "");
}

#[test]
fn a_whitespace_only_stream_stays_empty() {
    // A tool that ends its silence with a newline is still silent. Catches a
    // deleted `trim`, which would send "\n" through as a non-empty body.
    assert_eq!(stream_detail("  \n\t "), "");
}

#[test]
fn surrounding_whitespace_is_trimmed_from_real_output() {
    assert_eq!(
        stream_detail("\n  fatal: bad config  \n"),
        "fatal: bad config"
    );
}

#[test]
fn control_characters_are_stripped_rather_than_passed_to_the_terminal() {
    // The whole reason this routes through `excerpt` rather than a byte
    // truncation: a tool's stderr can carry an escape sequence, and the
    // predecessor passed it through unchanged.
    let out = stream_detail("error \u{1b}[31mred\u{7} here");
    assert!(!out.chars().any(char::is_control), "{out:?}");
    assert!(out.contains("red"), "{out:?}");
}

#[test]
fn long_output_is_bounded_and_marked() {
    let out = stream_detail(&"x".repeat(500));
    assert!(out.ends_with('…'), "{out:?}");
    assert_eq!(out.chars().count(), 201, "{out:?}");
}

#[test]
fn the_limit_counts_characters_not_bytes() {
    // Catches a bound applied to `len()` rather than to a character count:
    // 150 three-byte characters are 450 bytes but well inside the limit.
    let out = stream_detail(&"日".repeat(150));
    assert!(!out.contains('…'), "{out:?}");
    assert_eq!(out.chars().count(), 150, "{out:?}");
}
