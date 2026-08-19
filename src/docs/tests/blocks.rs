//! The three multi-line checks.

use crate::docs::tests::{fires_once_at, of_kind, positions, run, silent};
use crate::docs::{BLANK_RUN_MAX, Check};

#[test]
fn blank_run_boundary_is_exclusive_and_reports_the_first_blank() {
    // Two blanks is fine, three is not - and the finding points at the first
    // blank of the run, which is line 2, not the line that tripped the count.
    silent("a\n\n\nb\n", Check::MultipleBlankLines);
    fires_once_at("a\n\n\n\nb\n", Check::MultipleBlankLines, 2, 1);
}

#[test]
fn a_long_run_reports_once_not_once_per_line() {
    fires_once_at("a\n\n\n\n\n\n\n\nb\n", Check::MultipleBlankLines, 2, 1);
}

#[test]
fn two_separate_runs_each_report() {
    let content = "a\n\n\n\nb\n\n\n\nc\n";
    assert_eq!(
        positions(content, Check::MultipleBlankLines),
        vec![(2, 1), (6, 1)]
    );
}

#[test]
fn whitespace_only_lines_count_as_blank() {
    fires_once_at("a\n   \n\t\n\nb\n", Check::MultipleBlankLines, 2, 1);
}

#[test]
fn a_fence_breaks_a_blank_run_rather_than_being_skipped_over() {
    // Two blanks, a code block, two more blanks. Merged into one run of four
    // this fires; treated as two runs of two it does not. "Skip the fenced
    // lines" and "reset at the fence" differ by exactly this case.
    let content = "a\n\n\n```text\ncode\n```\n\n\nb\n";
    silent(content, Check::MultipleBlankLines);
    // The same shape without the fence does fire, so the assertion above is
    // about the fence and not about the counting being broken.
    assert_eq!(
        of_kind("a\n\n\n\n\n\nb\n", Check::MultipleBlankLines).len(),
        1
    );
}

#[test]
fn blank_lines_inside_a_fence_are_not_a_run() {
    let content = "```text\n\n\n\n\n```\n";
    silent(content, Check::MultipleBlankLines);
}

#[test]
fn the_run_threshold_comes_from_the_constant() {
    // Derived from `BLANK_RUN_MAX` rather than written as 3, so raising the
    // constant does not leave a test asserting the old number.
    let ok = format!("a\n{}b\n", "\n".repeat(BLANK_RUN_MAX));
    silent(&ok, Check::MultipleBlankLines);
    let bad = format!("a\n{}b\n", "\n".repeat(BLANK_RUN_MAX + 1));
    assert_eq!(of_kind(&bad, Check::MultipleBlankLines).len(), 1);
}

#[test]
fn a_file_ending_in_one_newline_has_no_trailing_blank_line() {
    // The terminator belongs to the last line of text. If this fired, every
    // well-formed file in the repository would report it.
    silent("a\nb\n", Check::TrailingBlankLines);
    silent("a", Check::TrailingBlankLines);
    silent("", Check::TrailingBlankLines);
}

#[test]
fn a_file_ending_in_a_blank_line_reports_at_that_line() {
    fires_once_at("a\n\n", Check::TrailingBlankLines, 2, 1);
    fires_once_at("a\n\n\n", Check::TrailingBlankLines, 3, 1);
}

#[test]
fn the_trailing_blank_count_is_the_run_not_the_file() {
    let found = of_kind("a\n\n\n", Check::TrailingBlankLines);
    assert!(found[0].message.contains('2'), "{}", found[0].message);
    let found = of_kind("a\n\n", Check::TrailingBlankLines);
    assert!(found[0].message.contains('1'), "{}", found[0].message);
}

#[test]
fn a_whitespace_only_final_line_is_a_trailing_blank() {
    fires_once_at("a\n   \n", Check::TrailingBlankLines, 2, 1);
}

#[test]
fn an_even_number_of_fences_is_closed() {
    silent("```a\nx\n```\n", Check::UnclosedCodeFence);
    silent(
        "```a\nx\n```\n\ntext\n\n```b\ny\n```\n",
        Check::UnclosedCodeFence,
    );
}

#[test]
fn an_odd_number_of_fences_reports_the_last_opener() {
    fires_once_at("```a\nx\n", Check::UnclosedCodeFence, 1, 1);
    // Three delimiters: the third is the unclosed one, not the first.
    fires_once_at("```a\nx\n```\n\n```b\ny\n", Check::UnclosedCodeFence, 5, 1);
}

#[test]
fn the_unclosed_fence_message_quotes_the_opener() {
    let found = of_kind("text\n\n```rust\nfn main() {}\n", Check::UnclosedCodeFence);
    assert!(found[0].message.contains("```rust"), "{}", found[0].message);
}

#[test]
fn structural_checks_report_a_column() {
    // All three anchor at column 1; a `None` column would make the renderer
    // drop the position entirely.
    for content in ["a\n\n\n\nb\n", "a\n\n", "```a\n"] {
        for finding in run(content) {
            assert!(finding.column.is_some(), "{}: {content:?}", finding.kind);
        }
    }
}
