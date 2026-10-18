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
