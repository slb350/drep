//! Raw inline HTML is not markdown.
//!
//! A URL in an attribute value (`<img src="https://...">`, `<a href=...>`) is
//! the markup's own address, and wrapping it in `[text](url)` would break the
//! tag. The tag is blanked as CommonMark's raw HTML grammar defines it, so each
//! case here sits on one side of that grammar: a malformed tag is plain text,
//! and a URL in plain text is still bare.

use crate::docs::Check;
use crate::docs::tests::{fires_once_at, of_kind, silent};

#[test]
fn a_url_in_a_quoted_attribute_is_not_bare() {
    silent(
        r#"<a href="https://example.com/x">docs</a>"#,
        Check::BareUrl,
    );
    silent("<a href='https://example.com/x'>docs</a>", Check::BareUrl);
    silent(
        r#"<img src="https://example.com/a.png" alt="logo" />"#,
        Check::BareUrl,
    );
}

#[test]
fn a_url_in_an_unquoted_attribute_is_not_bare() {
    silent("<img src=https://example.com/a.png>", Check::BareUrl);
    silent("<a href=https://example.com>docs</a>", Check::BareUrl);
}

#[test]
fn whitespace_around_the_equals_sign_is_part_of_the_tag() {
    silent(r#"<a href = "https://example.com">x</a>"#, Check::BareUrl);
}

#[test]
fn tag_and_attribute_names_follow_the_grammar() {
    silent(r#"<A HREF="https://example.com">x</A>"#, Check::BareUrl);
    silent(r#"<h1 id="https://example.com">x</h1>"#, Check::BareUrl);
    silent(
        r#"<my-element a="https://example.com">x</my-element>"#,
        Check::BareUrl,
    );
    silent(
        r#"<svg xlink:href="https://example.com" data-x.y_z="1">"#,
        Check::BareUrl,
    );
    silent(r#"<a _x="https://example.com">x</a>"#, Check::BareUrl);
}

#[test]
fn a_greater_than_sign_inside_a_quoted_value_does_not_end_the_tag() {
    silent(
        r#"<a title="a > b" href="https://example.com">x</a>"#,
        Check::BareUrl,
    );
}

#[test]
fn a_valueless_attribute_before_a_url_attribute_keeps_the_tag_whole() {
    silent(
        r#"<a download href="https://example.com">x</a>"#,
        Check::BareUrl,
    );
}

#[test]
fn only_the_tag_is_blanked_not_the_text_around_it() {
    // Column 17 is the `h`: the tags before it are spaces, the text is not.
    fires_once_at("<b>bold</b> see https://example.com", Check::BareUrl, 1, 17);
    fires_once_at("<br/> https://example.com", Check::BareUrl, 1, 7);
    fires_once_at("<br /> https://example.com", Check::BareUrl, 1, 8);
    // A URL between tags is text, and its report stops at the closing tag.
    fires_once_at("<b>https://example.com</b>", Check::BareUrl, 1, 4);
    let found = of_kind("<b>https://example.com</b>", Check::BareUrl);
    assert_eq!(found[0].message, "bare URL: https://example.com");
}

#[test]
fn a_closing_tag_may_hold_whitespace_before_its_bracket() {
    fires_once_at("</a > https://example.com", Check::BareUrl, 1, 7);
    fires_once_at("</a> https://example.com", Check::BareUrl, 1, 6);
}

#[test]
fn a_closing_tag_takes_no_attributes() {
    fires_once_at(r#"</a href="https://example.com">"#, Check::BareUrl, 1, 11);
}

#[test]
fn a_less_than_sign_that_opens_no_tag_is_plain_text() {
    fires_once_at("a < b and https://example.com > c", Check::BareUrl, 1, 11);
    fires_once_at(r#"<1a href="https://example.com">"#, Check::BareUrl, 1, 11);
    fires_once_at(r#"<-a href="https://example.com">"#, Check::BareUrl, 1, 11);
    fires_once_at("<> https://example.com", Check::BareUrl, 1, 4);
    fires_once_at("</> https://example.com", Check::BareUrl, 1, 5);
    fires_once_at("</1> https://example.com", Check::BareUrl, 1, 6);
}

#[test]
fn a_url_is_not_an_attribute_name() {
    // `https:` is a legal name, `//example.com` is not, so this is not a tag
    // and the URL in it is as bare as any other.
    fires_once_at("x<y https://example.com>", Check::BareUrl, 1, 5);
}

#[test]
fn attributes_need_whitespace_between_them() {
    fires_once_at(
        r#"<a href="x"href="https://example.com">"#,
        Check::BareUrl,
        1,
        18,
    );
    // A name must follow the tag name after whitespace, not run into it.
    fires_once_at(r#"<a:b href="https://example.com">"#, Check::BareUrl, 1, 12);
}

#[test]
fn an_unterminated_tag_is_plain_text() {
    // A self-closing slash must be followed by the bracket.
    fires_once_at(
        r#"<a download/ href="https://example.com">"#,
        Check::BareUrl,
        1,
        20,
    );
    fires_once_at(r#"<a href="https://example.com"#, Check::BareUrl, 1, 10);
    fires_once_at(r#"<a href="https://example.com>"#, Check::BareUrl, 1, 10);
    fires_once_at("<a href=https://example.com", Check::BareUrl, 1, 9);
    fires_once_at("<a href= https://example.com", Check::BareUrl, 1, 10);
    fires_once_at("<a/ https://example.com>", Check::BareUrl, 1, 5);
    fires_once_at("</a https://example.com>", Check::BareUrl, 1, 5);
}

#[test]
fn an_unquoted_value_ends_at_the_first_character_the_grammar_forbids() {
    fires_once_at(r#"<a href=x"y https://example.com>"#, Check::BareUrl, 1, 13);
    fires_once_at("<a href=x=y https://example.com>", Check::BareUrl, 1, 13);
    fires_once_at("<a href=x<y https://example.com>", Check::BareUrl, 1, 13);
    fires_once_at("<a href=x`y https://example.com>", Check::BareUrl, 1, 13);
}

#[test]
fn an_html_comment_is_not_markdown() {
    silent("<!-- see https://example.com -->", Check::BareUrl);
    fires_once_at("<!-- note --> https://example.com", Check::BareUrl, 1, 15);
    fires_once_at("<!-- see https://example.com", Check::BareUrl, 1, 10);
}

#[test]
fn brackets_inside_a_tag_do_not_unbalance_the_line() {
    silent(r#"<span title="[draft">x</span>"#, Check::LinkSyntaxInvalid);
    silent("<!-- ]( -->", Check::LinkSyntaxInvalid);
}

#[test]
fn brackets_around_a_tag_still_count() {
    fires_once_at(
        r#"<span title="x">[draft</span>"#,
        Check::LinkSyntaxInvalid,
        1,
        1,
    );
}

#[test]
fn an_autolink_is_still_an_autolink_beside_a_tag() {
    silent("<b>x</b> <https://example.com>", Check::BareUrl);
}

#[test]
fn a_generic_type_name_is_a_tag_and_hides_nothing_outside_it() {
    fires_once_at("Vec<T> is at https://example.com", Check::BareUrl, 1, 14);
    fires_once_at("<T: Display> https://example.com", Check::BareUrl, 1, 14);
}

#[test]
fn the_shortest_comments_are_complete() {
    // CommonMark 0.31.2: `<!-->` and `<!--->` are whole comments, so the text
    // after them is not inside one.
    fires_once_at("<!--> https://example.com", Check::BareUrl, 1, 7);
    fires_once_at("<!---> https://example.com", Check::BareUrl, 1, 8);
    fires_once_at("<!----> https://example.com", Check::BareUrl, 1, 9);
    fires_once_at("<!- https://example.com -->", Check::BareUrl, 1, 5);
    fires_once_at("<! https://example.com -->", Check::BareUrl, 1, 4);
}

#[test]
fn an_attribute_name_may_start_with_a_colon() {
    silent(r#"<a :href="https://example.com">x</a>"#, Check::BareUrl);
}

#[test]
fn a_comment_ends_at_the_first_close_after_its_opener_wherever_it_starts() {
    // `<!-->` and `<!--->` close themselves even after other text, so the URL
    // after them is prose and a later `-->` closes nothing.
    fires_once_at("See <!--> https://example.com -->", Check::BareUrl, 1, 11);
    fires_once_at("See <!---> https://example.com -->", Check::BareUrl, 1, 12);
}

#[test]
fn an_empty_unquoted_value_ends_the_tag_as_plain_text() {
    // An unquoted value is nonempty, so `href=>` is not an attribute and the
    // earlier attribute's URL is not inside a tag.
    fires_once_at(
        r#"<a title="https://example.com" href=>"#,
        Check::BareUrl,
        1,
        11,
    );
}

#[test]
fn only_spaces_and_tabs_separate_the_parts_of_a_tag() {
    silent("<a\thref=\"https://example.com\">x</a>", Check::BareUrl);
    // A no-break space is whitespace to Rust but not to the grammar, so this
    // is not a tag and the URL in it is reported.
    fires_once_at(
        "<a\u{a0}href=\"https://example.com\">",
        Check::BareUrl,
        1,
        10,
    );
}

// A tag or comment may run over several lines: CommonMark counts a line
// ending as whitespace between a tag's parts, and a quoted value or a comment
// may contain one. The cases below fix where such a construct starts and ends
// by what is reported around it, and which line ending does not continue it.

#[test]
fn a_url_in_an_attribute_on_a_later_line_is_not_bare() {
    silent(
        "<img alt=\"logo\"\n     src=\"https://example.com/a.png\">",
        Check::BareUrl,
    );
    silent(
        "<a\n  href=https://example.com\n  class=x>docs</a>",
        Check::BareUrl,
    );
}

#[test]
fn a_url_in_an_attribute_on_the_opening_line_is_not_bare() {
    silent(
        "<a href=\"https://example.com/x\"\n   class=\"link\">docs</a>",
        Check::BareUrl,
    );
}

#[test]
fn a_quoted_value_may_hold_a_line_ending() {
    silent(
        "<a title=\"first\nsecond\" href=\"https://example.com\">x</a>",
        Check::BareUrl,
    );
}

#[test]
fn text_after_a_multi_line_tag_is_still_markdown() {
    // Line 2 is `     alt="y"> https://example.com`: the `h` is column 15.
    fires_once_at(
        "<img src=\"x\"\n     alt=\"y\"> https://example.com",
        Check::BareUrl,
        2,
        15,
    );
    // And text before the tag on its first line.
    fires_once_at(
        "see https://example.com <img\n alt=\"y\">",
        Check::BareUrl,
        1,
        5,
    );
}

#[test]
fn a_multi_line_tag_in_the_middle_of_a_paragraph_hides_only_itself() {
    let doc = "one https://a.example.com\n<img src=\"https://b.example.com\"\n alt=\"y\"> two\nthree https://c.example.com";
    assert_eq!(
        crate::docs::tests::positions(doc, Check::BareUrl),
        vec![(1, 5), (4, 7)]
    );
}

#[test]
fn a_tag_does_not_continue_over_a_blank_line() {
    // The paragraph ends at the blank line, so the tag never closes.
    fires_once_at(
        "<img alt=\"x\"\n\nsrc=\"https://example.com\">",
        Check::BareUrl,
        3,
        6,
    );
    // A line of spaces and tabs is blank too.
    fires_once_at(
        "<img alt=\"x\"\n \t \nsrc=\"https://example.com\">",
        Check::BareUrl,
        3,
        6,
    );
}

#[test]
fn a_tag_does_not_continue_into_or_out_of_a_code_fence() {
    fires_once_at(
        "<img alt=\"x\"\n```\nfence\n```\nsrc=https://example.com>",
        Check::BareUrl,
        5,
        5,
    );
}

#[test]
fn a_multi_line_tag_that_is_never_closed_is_plain_text() {
    fires_once_at(
        "<img alt=\"x\"\n src=\"https://example.com\"",
        Check::BareUrl,
        2,
        7,
    );
}

#[test]
fn brackets_inside_a_multi_line_tag_do_not_unbalance_either_line() {
    silent(
        "<span title=\"[draft\"\n      data-x=\"](\">x</span>",
        Check::LinkSyntaxInvalid,
    );
}

#[test]
fn a_code_span_hides_a_comment_opener_from_the_multi_line_scan() {
    // The backticked `<!--` opens no comment, so the URL after it is prose.
    fires_once_at(
        "Use `<!--` to open https://example.com and `-->` to close.",
        Check::BareUrl,
        1,
        20,
    );
    fires_once_at(
        "Use `<!--` to open\nhttps://example.com and `-->` to close.",
        Check::BareUrl,
        2,
        1,
    );
}

#[test]
fn a_comment_over_several_lines_is_not_markdown() {
    silent(
        "<!--\nsee https://example.com\nand [broken](\n-->",
        Check::BareUrl,
    );
    silent(
        "<!--\nsee https://example.com\nand [broken](\n-->",
        Check::LinkSyntaxInvalid,
    );
    silent(
        "text <!-- see\nhttps://example.com\n--> more",
        Check::BareUrl,
    );
}

#[test]
fn text_after_a_multi_line_comment_is_still_markdown() {
    fires_once_at("<!--\nnote\n--> https://example.com", Check::BareUrl, 3, 5);
    fires_once_at("https://example.com <!--\nnote -->", Check::BareUrl, 1, 1);
}

#[test]
fn a_comment_that_starts_a_line_may_hold_blank_lines() {
    silent(
        "<!--\nfirst https://a.example.com\n\nsecond https://b.example.com\n-->",
        Check::BareUrl,
    );
    silent("   <!-- x\n\nhttps://example.com\n-->", Check::BareUrl);
    // Text on the opening and closing lines is inside the comment too, up to the `-->`.
    silent(
        "<!-- see https://a.example.com\n\nmore\nhttps://b.example.com -->",
        Check::BareUrl,
    );
    // The comment closes on the first `-->` and the paragraph after it is prose.
    fires_once_at(
        "<!--\nnote\n\nmore -->\n\nhttps://example.com",
        Check::BareUrl,
        6,
        1,
    );
}

#[test]
fn a_comment_opening_four_spaces_in_is_not_a_block_comment() {
    // Indented code is not modelled, so this stays the ordinary inline rule:
    // a comment that has not closed by the end of its paragraph is text.
    fires_once_at(
        "    <!-- x\n\nhttps://example.com\n-->",
        Check::BareUrl,
        3,
        1,
    );
}

#[test]
fn a_comment_after_text_does_not_continue_over_a_blank_line() {
    fires_once_at(
        "text <!-- x\n\nhttps://example.com -->",
        Check::BareUrl,
        3,
        1,
    );
}

#[test]
fn a_comment_that_closes_on_its_opening_line_does_not_swallow_what_follows() {
    fires_once_at(
        "<!-- x -->\n\nhttps://example.com\n-->",
        Check::BareUrl,
        3,
        1,
    );
    fires_once_at("<!-->\n\nhttps://example.com\n-->", Check::BareUrl, 3, 1);
    fires_once_at("<!--->\n\nhttps://example.com\n-->", Check::BareUrl, 3, 1);
}

#[test]
fn a_comment_that_is_never_closed_is_plain_text() {
    fires_once_at("<!--\n\nhttps://example.com", Check::BareUrl, 3, 1);
}

#[test]
fn a_wide_character_before_a_multi_line_tag_keeps_columns_in_characters() {
    // `é` is two bytes; the `h` after `> ` on line 2 is the 14th character.
    fires_once_at(
        "<a\n   href=\"é\"> https://example.com",
        Check::BareUrl,
        2,
        14,
    );
}

#[test]
fn a_block_comment_may_follow_another_directly() {
    silent(
        "<!--\n\nhttps://a.example.com\n-->\n<!--\n\nhttps://b.example.com\n-->",
        Check::BareUrl,
    );
    fires_once_at(
        "<!--\n\nhttps://a.example.com\n-->\nhttps://b.example.com",
        Check::BareUrl,
        5,
        1,
    );
}

#[test]
fn a_tag_holding_a_code_span_over_a_line_ending_is_still_blanked_on_both_lines() {
    // Backticks are ordinary characters inside a quoted value, so the tag is
    // whole and the URL on its second line is its own attribute.
    silent(
        "<a title=\"x `y\nz` w\" href=\"https://example.com\">",
        Check::BareUrl,
    );
}

// `html::spans` is checked directly as well: a tag's closing `>` is never part
// of a URL or a bracket count, so no finding can show whether it was covered.

/// The raw HTML ranges `html::spans` reports for each line of `content`.
fn spans_of(content: &str) -> Vec<Vec<std::ops::Range<usize>>> {
    let lines: Vec<crate::docs::Line<'_>> = content
        .lines()
        .zip(1..)
        .map(|(text, number)| crate::docs::Line {
            number,
            text,
            in_fence: false,
        })
        .collect();
    crate::docs::html::spans(&lines)
}

#[test]
fn a_span_covers_a_tag_through_its_closing_bracket() {
    assert_eq!(spans_of("<b>x</b>"), vec![vec![0..3, 4..8]]);
    assert_eq!(spans_of("a <!-- c --> b"), vec![vec![2..12]]);
}

#[test]
fn a_span_over_a_line_ending_is_split_at_it() {
    // Line 1 is `x <img`, line 2 is ` src="y"> z`: each keeps only its own
    // columns, and the second ends after the `>`.
    assert_eq!(
        spans_of("x <img\n src=\"y\"> z"),
        vec![vec![2..6], vec![0..9]]
    );
}
