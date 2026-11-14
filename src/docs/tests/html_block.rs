//! HTML blocks (CommonMark 0.31.2, section 4.6).
//!
//! A line that starts an HTML block is raw HTML up to the block's end
//! condition: nothing in it is a heading or a link, so a `#` line inside
//! `<pre>` is not a malformed heading and a URL in `<div>` text is not a bare
//! one. Each case sits on one side of a start or end condition, and the pairs
//! pair a silent half with a firing half so a check that never fires cannot
//! pass on the silent one alone.

use crate::docs::Check;
use crate::docs::tests::{fires_once_at, of_kind, silent};

const URL: &str = "https://example.com/x";

#[test]
fn a_pre_block_is_raw_whatever_it_holds() {
    silent(
        &format!("<pre>\n#Heading\nsee {URL}\n[a](b\n</pre>"),
        Check::BareUrl,
    );
    silent("<pre>\n#Heading\n</pre>", Check::MissingSpaceAfterHeading);
    silent("<pre>\n#\n</pre>", Check::EmptyHeading);
    silent("<pre>\n[a](b\n</pre>", Check::LinkSyntaxInvalid);
}

#[test]
fn the_text_after_a_raw_block_is_markdown() {
    fires_once_at(&format!("<pre>\nx\n</pre>\n{URL}"), Check::BareUrl, 4, 1);
    fires_once_at(
        "<pre>\nx\n</pre>\n#Heading",
        Check::MissingSpaceAfterHeading,
        4,
        2,
    );
}

#[test]
fn blank_lines_do_not_end_a_script_style_pre_or_textarea_block() {
    for name in ["pre", "script", "style", "textarea"] {
        silent(
            &format!("<{name}>\nfirst\n\nsee {URL}\n</{name}>"),
            Check::BareUrl,
        );
    }
}

#[test]
fn the_end_tag_of_a_raw_block_need_not_match_and_ignores_case() {
    silent(&format!("<pre>\n{URL}\n</SCRIPT>"), Check::BareUrl);
    silent(&format!("<SCRIPT>\n{URL}\n</textarea>"), Check::BareUrl);
    silent(&format!("<style>\n{URL}\nx</style> y"), Check::BareUrl);
}

#[test]
fn a_raw_block_whose_end_tag_is_on_its_opening_line_is_one_line() {
    fires_once_at(&format!("<pre>x</pre>\n\n{URL}"), Check::BareUrl, 3, 1);
    fires_once_at(&format!("<pre>{URL}</pre>\n{URL}"), Check::BareUrl, 2, 1);
}

#[test]
fn a_raw_block_is_not_started_by_a_longer_name() {
    // The end tag is there, so only the name decides.
    fires_once_at(&format!("<prefix> y\n{URL}\n</pre>"), Check::BareUrl, 2, 1);
    fires_once_at(
        &format!("<scripture> y\n{URL}\n</script>"),
        Check::BareUrl,
        2,
        1,
    );
}

#[test]
fn a_raw_block_starts_before_a_space_a_tab_a_bracket_or_the_line_end() {
    silent(&format!("<pre class=a>\n{URL}\n</pre>"), Check::BareUrl);
    silent(&format!("<pre\tclass=a>\n{URL}\n</pre>"), Check::BareUrl);
    silent(&format!("<pre\n{URL}\n</pre>"), Check::BareUrl);
}

#[test]
fn a_raw_block_that_is_never_closed_is_plain_text() {
    // Rendered, it would run to the end of the file; read that way, one stray
    // `<pre` would silence every later check, so the lines stay prose.
    fires_once_at(&format!("<pre>\nx\n\n{URL}"), Check::BareUrl, 4, 1);
}

#[test]
fn a_processing_instruction_runs_to_its_closing_marker() {
    silent(&format!("<?php\n\n{URL}\n?>"), Check::BareUrl);
    fires_once_at(&format!("<?php ?>\n\n{URL}"), Check::BareUrl, 3, 1);
}

#[test]
fn a_declaration_runs_to_the_first_bracket() {
    silent(&format!("<!DOCTYPE\n\n{URL}\nhtml>"), Check::BareUrl);
    fires_once_at(&format!("<!DOCTYPE html>\n\n{URL}"), Check::BareUrl, 3, 1);
}

#[test]
fn a_declaration_needs_a_letter_after_the_bang() {
    fires_once_at(&format!("<! x\n{URL}\n>"), Check::BareUrl, 2, 1);
}

#[test]
fn a_cdata_section_runs_to_its_closing_marker() {
    silent(&format!("<![CDATA[\n\n{URL}\n]]>"), Check::BareUrl);
    fires_once_at(&format!("<![CDATA[ x ]]>\n\n{URL}"), Check::BareUrl, 3, 1);
}

#[test]
fn a_block_level_tag_starts_a_block_that_ends_at_a_blank_line() {
    silent(&format!("<div>\nsee {URL}\n</div>"), Check::BareUrl);
    silent("<div>\n#Heading\n</div>", Check::MissingSpaceAfterHeading);
    silent("<div>\n[a](b\n</div>", Check::LinkSyntaxInvalid);
    fires_once_at(&format!("<div>\nx\n\n{URL}"), Check::BareUrl, 4, 1);
}

#[test]
fn a_block_level_tag_may_be_closing_uppercase_or_self_closing() {
    silent(&format!("</div>\n{URL}"), Check::BareUrl);
    silent(&format!("<DIV>\n{URL}"), Check::BareUrl);
    silent(&format!("<hr/>\n{URL}"), Check::BareUrl);
    silent(&format!("<hr>\n{URL}"), Check::BareUrl);
    silent(&format!("<details open>\n{URL}"), Check::BareUrl);
    silent(&format!("<h1\n{URL}"), Check::BareUrl);
    silent(&format!("<td\tx>\n{URL}"), Check::BareUrl);
}

#[test]
fn a_block_level_name_is_whole() {
    // `divider` and `p-x` are not `div` and `p`, and text after the tag stops
    // them being the type 7 tag-only line as well.
    fires_once_at(&format!("<divider> {URL}"), Check::BareUrl, 1, 11);
    fires_once_at(&format!("<p-x> {URL}"), Check::BareUrl, 1, 7);
    fires_once_at(&format!("<h7> {URL}"), Check::BareUrl, 1, 6);
}

#[test]
fn a_block_level_tag_interrupts_a_paragraph() {
    silent(&format!("text\n<div>\n{URL}"), Check::BareUrl);
    fires_once_at(&format!("text\n{URL}\n<div>"), Check::BareUrl, 2, 1);
}

#[test]
fn a_tag_alone_on_its_line_starts_a_block() {
    silent(&format!("<span>\n{URL}"), Check::BareUrl);
    silent(&format!("<span class=\"a\"> \t\n{URL}"), Check::BareUrl);
    silent(&format!("</span>\n{URL}"), Check::BareUrl);
    silent(&format!("<custom-tag/>\n{URL}"), Check::BareUrl);
    fires_once_at(&format!("<span>\n\n{URL}"), Check::BareUrl, 3, 1);
}

#[test]
fn a_tag_followed_by_text_does_not_start_a_block() {
    fires_once_at(&format!("<span> {URL}"), Check::BareUrl, 1, 8);
    fires_once_at(&format!("<span> y\n{URL}"), Check::BareUrl, 2, 1);
}

#[test]
fn an_incomplete_tag_does_not_start_a_block() {
    fires_once_at(&format!("<span class=\n{URL}"), Check::BareUrl, 2, 1);
}

#[test]
fn a_tag_alone_on_its_line_cannot_interrupt_a_paragraph() {
    fires_once_at(&format!("text\n<span>\n{URL}"), Check::BareUrl, 3, 1);
    fires_once_at(&format!("text\n</span>\n{URL}"), Check::BareUrl, 3, 1);
}

#[test]
fn a_tag_alone_on_its_line_may_follow_a_block_or_a_fence() {
    silent(&format!("<pre>x</pre>\n<span>\n{URL}"), Check::BareUrl);
    silent(&format!("```\nx\n```\n<span>\n{URL}"), Check::BareUrl);
    silent(&format!("<!-- x -->\n<span>\n{URL}"), Check::BareUrl);
    silent(&format!("text\n\n<span>\n{URL}"), Check::BareUrl);
}

#[test]
fn an_unclosed_raw_tag_is_not_a_tag_alone_on_its_line() {
    // The four raw names are excluded from the tag-alone rule, so an unclosed
    // one has no other way to start a block.
    fires_once_at(&format!("<textarea>\n{URL}"), Check::BareUrl, 2, 1);
}

#[test]
fn up_to_three_columns_of_indentation_still_start_a_block() {
    silent(&format!("   <div>\n{URL}"), Check::BareUrl);
    silent(&format!(" <pre>\n{URL}\n</pre>"), Check::BareUrl);
}

#[test]
fn four_columns_of_indentation_are_code_not_a_block() {
    fires_once_at(&format!("    <div>\n{URL}"), Check::BareUrl, 2, 1);
    fires_once_at(&format!("Intro.\n\n\t<div>\n{URL}"), Check::BareUrl, 4, 1);
}

#[test]
fn a_block_start_inside_a_code_fence_is_code() {
    fires_once_at(
        &format!("```\n<pre>\n```\n{URL}\n</pre>"),
        Check::BareUrl,
        4,
        1,
    );
    fires_once_at(&format!("~~~\n<div>\n~~~\n{URL}"), Check::BareUrl, 4, 1);
}

#[test]
fn a_block_start_inside_a_closed_comment_is_comment() {
    // Without skipping the comment, the `<pre>` in it would run to the `</pre>`
    // past the comment and hide the URL between them.
    fires_once_at(
        &format!("<!--\n<pre>\n-->\n\n{URL}\n</pre>"),
        Check::BareUrl,
        5,
        1,
    );
}

#[test]
fn a_comment_that_is_never_closed_does_not_hide_a_later_block() {
    silent(&format!("<!--\n\n<div>\n{URL}"), Check::BareUrl);
}

#[test]
fn an_html_block_does_not_silence_hygiene_checks() {
    fires_once_at("<div>\nx \n</div>", Check::TrailingWhitespace, 2, 2);
    fires_once_at("<div>\nx\ty\n</div>", Check::TabCharacter, 2, 2);
}

#[test]
fn html_blocks_are_found_one_after_another() {
    let content = format!("<div>\n{URL}\n\n{URL}\n\n<pre>\n{URL}\n</pre>\n{URL}");
    let lines: Vec<u32> = of_kind(&content, Check::BareUrl)
        .iter()
        .map(|f| f.line)
        .collect();
    assert_eq!(lines, vec![4, 9]);
}

#[test]
fn a_block_may_end_the_file() {
    silent(&format!("<div>\n{URL}"), Check::BareUrl);
    silent(&format!("<div>\n{URL}\n"), Check::BareUrl);
}

#[test]
fn the_shortest_comments_close_on_their_opening_line() {
    // Each is complete where it stands, so the `<pre>` after it is a block of
    // its own; read as open until the later `-->`, the comment would swallow it.
    for opener in ["<!-->", "<!--->", "<!-- x -->"] {
        silent(
            &format!("{opener}\n<pre>\n-->\n\n{URL}\n</pre>"),
            Check::BareUrl,
        );
    }
}
