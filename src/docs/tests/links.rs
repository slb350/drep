//! The two link checks, and the blanking they both rest on.
//!
//! Most of these are false-positive tests. Both checks are cheap to make fire
//! and expensive to make *not* fire on ordinary prose, which is where every
//! historical bug in this pair lived.

use crate::docs::Check;
use crate::docs::tests::{fires_once_at, of_kind, silent};

// ---- bare_url ----

#[test]
fn a_bare_url_reports_at_its_first_character() {
    fires_once_at("see https://example.com/x for more", Check::BareUrl, 1, 5);
    fires_once_at("http://example.com", Check::BareUrl, 1, 1);
}

#[test]
fn a_url_inside_a_well_formed_link_is_not_bare() {
    silent("[example](https://example.com)", Check::BareUrl);
    silent(
        "see [the docs](http://example.com/a/b) here",
        Check::BareUrl,
    );
}

#[test]
fn blanking_a_code_span_leaves_the_text_before_it_alone() {
    // Every other inline-code test has the span at the start of the line or
    // covering the whole line, so an implementation that blanked from column 1
    // up to the first backtick passed all of them. This is the case that
    // separates "blank the span" from "blank everything up to the span".
    fires_once_at("see https://example.com and `code`", Check::BareUrl, 1, 5);
}

#[test]
fn a_url_inside_backticks_is_not_bare() {
    // Inline code is a literal a reader is meant to type. Wrapping it in a
    // markdown link would change what they type.
    silent("run `curl https://example.com`", Check::BareUrl);
}

#[test]
fn one_link_does_not_excuse_a_bare_url_beside_it() {
    // The whole reason the line is *blanked* rather than tested for
    // "contains a link". A `contains` guard passes every other test in this
    // file and fails this one.
    fires_once_at(
        "[docs](https://example.com) and https://bare.example.com",
        Check::BareUrl,
        1,
        33,
    );
}

#[test]
fn a_scheme_with_nothing_after_it_is_not_a_url() {
    // Prose naming the scheme itself. There is nothing here to link to.
    silent("the prefix https:// is required", Check::BareUrl);
    silent("https://", Check::BareUrl);
}

#[test]
fn the_url_ends_at_whitespace() {
    let found = of_kind("go to https://example.com/a now", Check::BareUrl);
    assert!(
        found[0].message.ends_with("https://example.com/a"),
        "{}",
        found[0].message
    );
}

#[test]
fn a_very_long_url_is_excerpted_rather_than_printed_whole() {
    // The message lands in a terminal and the URL comes from a file drep does
    // not control.
    let url = format!("https://example.com/{}", "a".repeat(500));
    let found = of_kind(&url, Check::BareUrl);
    assert!(
        found[0].message.chars().count() < 90,
        "{}",
        found[0].message
    );
    assert!(found[0].message.ends_with('…'), "{}", found[0].message);
    // A short one is printed intact, so the truncation is conditional.
    let found = of_kind("https://example.com", Check::BareUrl);
    assert_eq!(found[0].message, "bare URL: https://example.com");
}

#[test]
fn a_url_carrying_an_escape_sequence_cannot_reach_the_terminal_intact() {
    // The URL is copied out of a file drep does not control and printed into a
    // terminal. This is what `text::excerpt` is for, and the local truncation
    // it replaced only shortened.
    let found = of_kind("https://example.com/\u{1b}[31mred", Check::BareUrl);
    assert!(
        !found[0].message.chars().any(char::is_control),
        "{:?}",
        found[0].message
    );
}

#[test]
fn only_the_first_bare_url_on_a_line_is_reported() {
    // One finding per line, matching 1.x. Two URLs on one line is one thing to
    // fix, and two findings at two columns reads as two.
    assert_eq!(
        of_kind("https://a.example and https://b.example", Check::BareUrl).len(),
        1
    );
}

// ---- link_syntax_invalid ----

#[test]
fn a_well_formed_link_is_silent() {
    silent("[text](url)", Check::LinkSyntaxInvalid);
    silent("a [one](x) and [two](y) b", Check::LinkSyntaxInvalid);
}

#[test]
fn a_badge_is_one_link_not_two_broken_ones() {
    // `[![alt](img)](href)`. A link scanner whose text may not contain
    // brackets stops at the image's `]` and leaves `](href)` looking broken -
    // which fires on the first line of most READMEs.
    silent(
        "[![build](img.svg)](https://ci.example)",
        Check::LinkSyntaxInvalid,
    );
}

#[test]
fn a_prose_parenthetical_is_not_broken_link_syntax() {
    // Counting parentheses over the raw line flags every sentence that opens a
    // parenthetical and closes it on the next line. The `](` guard is what
    // stops it.
    silent("a sentence (that wraps onto", Check::LinkSyntaxInvalid);
    silent("continued) here", Check::LinkSyntaxInvalid);
    silent("a smiley :-) alone", Check::LinkSyntaxInvalid);
}

#[test]
fn an_unclosed_link_target_is_reported() {
    fires_once_at("[text](url", Check::LinkSyntaxInvalid, 1, 1);
}

#[test]
fn an_unbalanced_bracket_is_reported_in_either_direction() {
    fires_once_at("[text without a close", Check::LinkSyntaxInvalid, 1, 1);
    fires_once_at("text] without an open", Check::LinkSyntaxInvalid, 1, 1);
}

#[test]
fn brackets_inside_backticks_do_not_count() {
    // A literal showing an array index is not a link.
    silent("use `items[0]` here", Check::LinkSyntaxInvalid);
    silent("write `[` to open", Check::LinkSyntaxInvalid);
}

#[test]
fn a_reference_style_link_is_balanced() {
    silent("[text][ref]", Check::LinkSyntaxInvalid);
}

#[test]
fn a_link_reference_definition_declares_a_url_rather_than_leaving_one_bare() {
    // The Keep a Changelog footer, which is nothing but these. 1.x reported
    // every one of them; this repository's own CHANGELOG.md has nine.
    silent(
        "[1.1.3]: https://github.com/slb350/drep/compare/v1.1.2...v1.1.3",
        Check::BareUrl,
    );
    silent("[1.1.3]: https://example.com", Check::LinkSyntaxInvalid);
    // Indented definitions inside a list still count.
    silent("  [ref]: https://example.com", Check::BareUrl);
}

#[test]
fn a_definition_only_exempts_its_own_destination() {
    // Prose that merely *starts* with a bracket is not a definition, and a
    // bare URL later on the same line as a definition is still bare... but a
    // definition's own trailing title is part of the declaration.
    fires_once_at(
        "[not a definition] https://example.com",
        Check::BareUrl,
        1,
        20,
    );
    silent(
        "[ref]: https://a.example \"see https://b.example\"",
        Check::BareUrl,
    );
}

#[test]
fn a_link_target_containing_parentheses_matches_to_the_first_close() {
    // Markdown's own parsers stop at the first `)`. Being stricter here would
    // report a false defect on every link to a disambiguation page.
    silent("[a](b(c)", Check::LinkSyntaxInvalid);
}

#[test]
fn an_unterminated_code_span_does_not_swallow_the_rest_of_the_line() {
    // A lone backtick is prose. Blanking from it to end-of-line would hide a
    // genuinely broken link sitting after it - the check would go quiet
    // exactly when the line is at its messiest.
    fires_once_at(
        "a ` stray tick then [broken](",
        Check::LinkSyntaxInvalid,
        1,
        1,
    );
}

#[test]
fn two_nested_brackets_in_a_row_terminate() {
    // `[x[a][b](u)`: the link-text scanner consumes `[a]` as a nested pair and
    // resumes on the `[` of `[b]`, which is the only shape where it enters the
    // nested branch twice running. An implementation that resumes one
    // character early re-finds the `]` it just consumed and never advances, so
    // this input hangs rather than reporting anything.
    let findings = of_kind("[x[a][b](u)", Check::LinkSyntaxInvalid);
    assert_eq!(findings.len(), 1, "{findings:?}");
    // `[x[a]` survives the blanking: two `[` against one `]`.
    assert!(
        findings[0].message.contains("2 `[` against 1 `]`"),
        "{}",
        findings[0].message
    );
}

#[test]
fn the_message_names_the_counts_that_disagree() {
    // The counts, not just the fact of a mismatch. A message that reads the
    // same for "one bracket too many" and "one too few" leaves the reader to
    // re-count the line by hand.
    let found = of_kind("[text without a close", Check::LinkSyntaxInvalid);
    assert!(
        found[0].message.contains("1 `[` against 0 `]`"),
        "{}",
        found[0].message
    );
    let found = of_kind("text] without an open", Check::LinkSyntaxInvalid);
    assert!(
        found[0].message.contains("0 `[` against 1 `]`"),
        "{}",
        found[0].message
    );
    let found = of_kind("[text](url", Check::LinkSyntaxInvalid);
    assert!(
        found[0].message.contains("1 `(` against 0 `)`"),
        "{}",
        found[0].message
    );
}

#[test]
fn a_definition_with_no_space_after_the_colon_still_declares_its_url() {
    // CommonMark makes the whitespace between `:` and the destination
    // optional. Finding the destination by looking for *whitespace* rather
    // than for the first non-whitespace character works on every spaced
    // definition and fails on this one, leaving the URL bare.
    silent("[ref]:https://example.com", Check::BareUrl);
}

#[test]
fn a_long_label_does_not_move_where_the_destination_starts() {
    // The destination begins two characters past the `]` - skipping the `]`
    // and the `:`. Any arithmetic that scales with the label's length instead
    // lands inside a long URL and leaves its first half looking bare.
    silent("[unreleased-notes]: https://example.com/x", Check::BareUrl);
}

#[test]
fn link_checks_report_the_right_line_number() {
    let content = "clean\n\n[broken](\n";
    fires_once_at(content, Check::LinkSyntaxInvalid, 3, 1);
}
