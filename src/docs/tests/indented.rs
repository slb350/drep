//! Indented code blocks (CommonMark 0.31.2, section 4.4).
//!
//! An indented chunk after a blank line is code, so the checks whose advice is
//! wrong inside a fence are wrong inside it too. Each case below is a document
//! that renders correctly. The pairs run the same snippet as prose and as code,
//! because a check that never fires would pass the silent half on its own.

use crate::docs::Check;
use crate::docs::tests::fence::{fence_blind, fence_sensitive};
use crate::docs::tests::{fires_once_at, of_kind, silent};

#[test]
fn fence_sensitive_checks_are_silent_in_an_indented_block() {
    for (check, snippet) in fence_sensitive() {
        silent(&format!("Intro.\n\n    {snippet}\n"), check);
    }
}

#[test]
fn fence_sensitive_checks_still_fire_in_the_prose_around_a_block() {
    for (check, snippet) in fence_sensitive() {
        let content = format!("{snippet}\n\n    code\n\n{snippet}\n");
        let lines: Vec<u32> = of_kind(&content, check).iter().map(|f| f.line).collect();
        assert_eq!(lines, vec![1, 5], "{} over {content:?}", check.as_str());
    }
}

#[test]
fn fence_blind_checks_fire_in_an_indented_block() {
    for (check, snippet) in fence_blind() {
        let content = format!("Intro.\n\n    {snippet}\n");
        fires_once_at(&content, check, 3, snippet.chars().count() as u32 + 4 - 1);
    }
}

#[test]
fn a_tab_indents_a_block() {
    silent("Intro.\n\n\tsee https://example.com/x\n", Check::BareUrl);
    silent("Intro.\n\n\tcol\tcol\n", Check::TabCharacter);
}

#[test]
fn a_block_starts_at_the_top_of_a_file() {
    silent("    see https://example.com/x\n", Check::BareUrl);
}

#[test]
fn a_block_runs_across_blank_lines() {
    silent(
        "Intro.\n\n    one\n\n    see https://example.com/x\n",
        Check::BareUrl,
    );
}

#[test]
fn a_block_ends_at_the_first_unindented_line() {
    fires_once_at(
        "Intro.\n\n    code\nsee https://example.com/x\n",
        Check::BareUrl,
        4,
        5,
    );
}

#[test]
fn three_spaces_do_not_make_a_block() {
    fires_once_at(
        "Intro.\n\n   see https://example.com/x\n",
        Check::BareUrl,
        3,
        8,
    );
}

#[test]
fn an_indented_line_continuing_a_paragraph_is_not_a_block() {
    // A block cannot interrupt a paragraph.
    fires_once_at(
        "Intro\n    see https://example.com/x\n",
        Check::BareUrl,
        2,
        9,
    );
}

#[test]
fn an_indented_paragraph_in_a_list_item_is_not_a_block() {
    // The item's content offset is not tracked, so under a list marker the
    // indentation is ambiguous and stays prose.
    fires_once_at(
        "- item\n\n    see https://example.com/x\n",
        Check::BareUrl,
        3,
        9,
    );
    fires_once_at(
        "1. item\n\n    see https://example.com/x\n",
        Check::BareUrl,
        3,
        9,
    );
}

#[test]
fn a_lazy_line_keeps_the_list_open() {
    fires_once_at(
        "- item\nlazy\n\n    see https://example.com/x\n",
        Check::BareUrl,
        4,
        9,
    );
}

#[test]
fn a_paragraph_after_a_list_closes_it() {
    silent(
        "- item\n\nText.\n\n    see https://example.com/x\n",
        Check::BareUrl,
    );
}

#[test]
fn a_marker_is_not_a_list_without_its_space() {
    silent("-item\n\n    see https://example.com/x\n", Check::BareUrl);
    silent(
        "1.5 apples\n\n    see https://example.com/x\n",
        Check::BareUrl,
    );
}

#[test]
fn a_block_after_a_closed_fence_is_a_block() {
    silent(
        "```text\nx\n```\n    see https://example.com/x\n",
        Check::BareUrl,
    );
}

#[test]
fn a_comment_opener_in_a_block_opens_nothing() {
    fires_once_at(
        "Intro.\n\n    <!-- x\n\nhttps://example.com\n-->\n",
        Check::BareUrl,
        5,
        1,
    );
}

#[test]
fn blank_lines_between_two_indented_lines_belong_to_the_block() {
    silent("Intro.\n\n    a\n\n\n\n    b\n", Check::MultipleBlankLines);
}

#[test]
fn blank_lines_after_a_block_are_still_a_run() {
    fires_once_at(
        "Intro.\n\n    a\n\n\n\ntext\n",
        Check::MultipleBlankLines,
        4,
        1,
    );
}

#[test]
fn blank_lines_between_two_fences_are_still_a_run() {
    fires_once_at(
        "```\na\n```\n\n\n\n```\nb\n```\n",
        Check::MultipleBlankLines,
        4,
        1,
    );
}

#[test]
fn a_tab_counts_to_the_next_tab_stop() {
    silent("Intro.\n\n  \tsee https://example.com/x\n", Check::BareUrl);
    silent("Intro.\n\n \t see https://example.com/x\n", Check::BareUrl);
    // Two spaces and a character are not four columns.
    fires_once_at(
        "Intro.\n\n  a https://example.com/x\n",
        Check::BareUrl,
        3,
        5,
    );
}

#[test]
fn every_list_marker_keeps_the_indentation_ambiguous() {
    for marker in [
        "- ",
        "* ",
        "+ ",
        "1. ",
        "1) ",
        "123456789. ",
        "-\t",
        "  - ",
        "   1. ",
    ] {
        let content = format!("{marker}item\n\n    see https://example.com/x\n");
        assert_eq!(
            of_kind(&content, Check::BareUrl).len(),
            1,
            "a `{marker}` line opens a list: {content:?}"
        );
    }
    // A marker with nothing after it is an empty item.
    for marker in ["-", "1."] {
        let content = format!("{marker}\n\n    see https://example.com/x\n");
        assert_eq!(of_kind(&content, Check::BareUrl).len(), 1, "{content:?}");
    }
}

#[test]
fn a_line_that_only_resembles_a_marker_opens_no_list() {
    for text in ["1234567890. item", "a. item", ". item", "1 item", "1.item"] {
        let content = format!("{text}\n\n    see https://example.com/x\n");
        silent(&content, Check::BareUrl);
    }
}

#[test]
fn indented_text_that_looks_like_a_marker_is_code_not_a_list() {
    silent(
        "Intro.\n\n    - a\n\n    see https://example.com/x\n",
        Check::BareUrl,
    );
}

#[test]
fn an_item_continuation_keeps_the_list_open_across_a_blank_line() {
    fires_once_at(
        "- a\n\n  more\n\n    see https://example.com/x\n",
        Check::BareUrl,
        5,
        9,
    );
}

#[test]
fn an_atx_heading_ends_the_paragraph_so_an_indented_line_is_code() {
    // A heading is a leaf block: no blank line is needed before the block.
    for heading in [
        "# Title",
        "###### Six",
        "   ## Indented",
        "#",
        "## Closed ##",
    ] {
        silent(
            &format!("{heading}\n    see https://example.com/x\n"),
            Check::BareUrl,
        );
    }
}

#[test]
fn text_that_only_resembles_a_heading_does_not_end_the_paragraph() {
    // Seven markers, no space after them and a four-column indent are not ATX
    // headings, so the indented line continues a paragraph.
    for text in ["####### Seven", "#hashtag", "\\# escaped"] {
        fires_once_at(
            &format!("{text}\n    see https://example.com/x\n"),
            Check::BareUrl,
            2,
            9,
        );
    }
}

#[test]
fn a_thematic_break_ends_the_paragraph_so_an_indented_line_is_code() {
    for rule in [
        "---", "***", "___", "- - -", " * * * *", "_\t_\t_", "-----   ",
    ] {
        silent(
            &format!("Text.\n{rule}\n    see https://example.com/x\n"),
            Check::BareUrl,
        );
    }
}

#[test]
fn two_marks_are_not_a_thematic_break() {
    // Each opens a paragraph after the blank line, so the indented line
    // continues it. Directly under a paragraph `--` would be a setext
    // underline instead (see the next test).
    fires_once_at(
        "Text.\n\n--\n    see https://example.com/x\n",
        Check::BareUrl,
        4,
        9,
    );
    // Mixed marks are not a break either.
    fires_once_at(
        "Text.\n\n-*-\n    see https://example.com/x\n",
        Check::BareUrl,
        4,
        9,
    );
}

#[test]
fn a_setext_underline_that_is_not_a_break_does_not_end_a_block_by_itself() {
    // CommonMark 0.31.2, section 4.3: an underline of any length turns the
    // paragraph above into a heading, so the next line could be code. Telling
    // an underline from a lazy list or quote continuation needs container
    // state drep does not track, so these stay text and the line stays prose.
    for underline in ["--", "-", "===", "="] {
        fires_once_at(
            &format!("Text.\n{underline}\n    see https://example.com/x\n"),
            Check::BareUrl,
            3,
            9,
        );
    }
}

#[test]
fn a_thematic_break_that_looks_like_a_list_item_opens_no_list() {
    silent("- - -\n\n    see https://example.com/x\n", Check::BareUrl);
}

#[test]
fn an_unindented_leaf_block_closes_a_list() {
    // Neither a heading nor a break can continue an item without indentation.
    silent(
        "- item\n# Title\n    see https://example.com/x\n",
        Check::BareUrl,
    );
    silent(
        "- item\n***\n    see https://example.com/x\n",
        Check::BareUrl,
    );
}

#[test]
fn an_indented_leaf_block_leaves_the_list_open() {
    fires_once_at(
        "- item\n  # Title\n    see https://example.com/x\n",
        Check::BareUrl,
        3,
        9,
    );
    fires_once_at(
        "- item\n\n  ***\n    see https://example.com/x\n",
        Check::BareUrl,
        4,
        9,
    );
}

#[test]
fn an_indented_hash_line_is_code_not_a_heading() {
    silent(
        "Intro.\n\n    # see https://example.com/x\n",
        Check::BareUrl,
    );
    silent(
        "Intro.\n\n    ---\n    see https://example.com/x\n",
        Check::BareUrl,
    );
}

#[test]
fn a_heading_line_inside_a_fence_ends_nothing() {
    // The `#` line is a shell comment: the list stays open around the fence.
    fires_once_at(
        "- item\n  ```sh\n# comment\n  ```\n\n    see https://example.com/x\n",
        Check::BareUrl,
        6,
        9,
    );
}

#[test]
fn blank_runs_at_either_end_of_a_file_are_runs() {
    fires_once_at("\n\n\n    a\n", Check::MultipleBlankLines, 1, 1);
    fires_once_at("    a\n\n\n\n", Check::MultipleBlankLines, 2, 1);
}

#[test]
fn indent_columns_expands_a_tab_to_the_next_stop() {
    // `mask` only compares the width against zero and four, and any tab within
    // the first four columns reaches four however it is counted, so no document
    // can tell an exact count from an inflated one. The width is pinned here.
    for (line, columns) in [
        ("", 0),
        ("x", 0),
        ("   x", 3),
        ("    x", 4),
        ("\tx", 4),
        (" \tx", 4),
        ("   \tx", 4),
        ("\t x", 5),
        ("    \tx", 8),
        ("\t\tx", 8),
    ] {
        assert_eq!(
            crate::docs::indented::indent_columns(line),
            columns,
            "{line:?}"
        );
    }
}
