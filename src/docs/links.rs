//! The two checks that need markdown's link grammar.
//!
//! Both work on a *blanked* copy of the line: inline code spans and
//! well-formed links are overwritten with spaces before either check looks at
//! it. Blanked rather than removed, so every surviving character keeps its
//! original column and a reported position points at the real line.
//!
//! The order is not arbitrary. Inline code is blanked first, because a
//! backticked `[text](url)` is a literal a reader is meant to type rather than
//! a link, so it must neither be consumed by the link scanner nor count toward
//! the bracket balance.

use crate::analysis::findings::Finding;
use crate::docs::{Check, Line, finding};
use crate::text::excerpt;

/// Longest URL echoed into a finding message.
///
/// The message lands in a terminal and the URL comes out of a file drep does
/// not control; a 2 KB data: URI would wrap the report off the screen, and a
/// URL holding an escape sequence would be interpreted by the terminal - which
/// is why the bounding goes through [`crate::text::excerpt`] rather than a
/// local truncation.
const URL_EXCERPT_MAX: usize = 60;

/// Run the link checks over `line`.
///
/// Skipped entirely inside a code fence: a sample showing `[broken](` on
/// purpose is documentation, not a defect.
pub fn check(
    line: &Line<'_>,
    chars: &[char],
    scratch: &mut Vec<char>,
    file_path: &str,
    out: &mut Vec<Finding>,
) {
    if line.in_fence {
        return;
    }

    // Blanking only copies when the line holds something to blank. A backtick
    // can open a code span and a `[` can open a link; a line with neither is
    // already its own blanked form. Lines holding a stray `]` and nothing else
    // fall into the else branch and are still counted below, which is the
    // point - an unmatched closing bracket is exactly what the balance check
    // exists to catch.
    //
    // `scratch` is owned by the caller and reused across lines, so the copy
    // costs a memcpy rather than an allocation.
    let blanked: &[char] = if line.text.contains('`') || line.text.contains('[') {
        scratch.clear();
        scratch.extend_from_slice(chars);
        blank_inline_code(scratch);
        blank_links(scratch);
        blank_reference_definition(scratch);
        scratch
    } else {
        chars
    };

    bare_url(line, blanked, file_path, out);
    link_syntax(line, blanked, file_path, out);
}

/// Overwrite every `` `code span` `` with spaces.
///
/// A backtick with no partner ends the scan: the rest of the line is prose, and
/// treating a lone backtick as opening a span that runs to end-of-line would
/// blank text that renders as itself.
fn blank_inline_code(chars: &mut [char]) {
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '`' {
            // From `i + 1`, not from `i`: `chars[i]` is the opening backtick,
            // and a search that could match it would pair every backtick with
            // itself and blank one character instead of the span.
            let Some(close) = (i + 1..chars.len()).find(|j| chars[*j] == '`') else {
                return;
            };
            chars[i..=close].fill(' ');
            i = close;
        }
        i += 1;
    }
}

/// Overwrite every well-formed `[text](url)` with spaces.
fn blank_links(chars: &mut [char]) {
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '[' {
            if let Some(end) = match_link(chars, i) {
                chars[i..=end].fill(' ');
                i = end;
            }
        }
        i += 1;
    }
}

/// Match a complete link starting at `start`, returning its last index.
///
/// The link text may contain one level of nested brackets, which is what makes
/// the badge construct `[![alt](img)](href)` match as a single link. Without
/// that, the scan stopped at the image's own `]` and left `](href)` looking
/// like broken syntax on every README with a badge in it.
///
/// The URL half is "everything up to the first `)`", so `[a](b(c)` matches
/// whole - markdown's own parsers are no stricter, and being stricter here
/// would report a false defect on a link to a Wikipedia disambiguation page.
fn match_link(chars: &[char], start: usize) -> Option<usize> {
    debug_assert_eq!(chars[start], '[');
    let mut j = start + 1;
    loop {
        match chars.get(j)? {
            // A nested `[...]`. The only thing a `[` can be here: the plain
            // alternative excludes both brackets, so a nested opener with no
            // closer fails the whole match rather than being skipped over.
            '[' => {
                // From `j`, not `j + 1`: `chars[j]` is `[`, so it can never be
                // the `]` being searched for, and starting a character earlier
                // would be an unobservable difference dressed up as a bound.
                // The `+ 1` that *is* load-bearing is the one below - resuming
                // at `close` would re-enter this loop on the `]` and take the
                // link-text-ends branch for a bracket already consumed.
                let close = (j..chars.len()).find(|k| chars[*k] == ']')?;
                j = close + 1;
            }
            // The end of the link text. Everything after it is fixed: `(`,
            // then a URL, then `)`. Nothing else can consume this `]`, so a
            // missing `(` fails the match outright.
            ']' => {
                if chars.get(j + 1) != Some(&'(') {
                    return None;
                }
                return (j + 2..chars.len()).find(|m| chars[*m] == ')');
            }
            _ => j += 1,
        }
    }
}

/// Overwrite the destination of a link reference definition with spaces.
///
/// `[ref]: https://example.com` declares a link target; the URL there is
/// *supposed* to be bare, and wrapping it in `[text](url)` would break the
/// definition. 1.x reported it, which meant nine spurious findings in this
/// repository's own `CHANGELOG.md` alone - the Keep a Changelog footer is
/// nothing but reference definitions, and so is the bottom of most READMEs.
///
/// Only the destination is blanked. The `[ref]` half stays, so the bracket
/// balance check still sees a well-formed pair and a genuinely broken
/// definition is still caught.
fn blank_reference_definition(chars: &mut [char]) {
    let Some(open) = chars.iter().position(|c| !c.is_whitespace()) else {
        return;
    };
    if chars[open] != '[' {
        return;
    }
    // A label holds no brackets of its own, so the first `]` ends it - and a
    // stray `[` inside means this is not a definition at all.
    let Some(close) = (open + 1..chars.len()).find(|i| matches!(chars[*i], '[' | ']')) else {
        return;
    };
    if chars[close] != ']' || chars.get(close + 1) != Some(&':') {
        return;
    }
    let Some(destination) = (close + 2..chars.len()).find(|i| !chars[*i].is_whitespace()) else {
        return;
    };
    chars[destination..].fill(' ');
}

/// A URL written as itself rather than as a link.
///
/// Reported against `blanked`, so a URL inside a well-formed link or inside
/// backticks is already spaces and does not fire. Reported *per line* rather
/// than per URL, matching 1.x - but the blanking is what makes that honest: one
/// link on the line does not excuse a bare URL sitting beside it, because only
/// the link's own characters were blanked.
fn bare_url(line: &Line<'_>, blanked: &[char], file_path: &str, out: &mut Vec<Finding>) {
    let Some(start) = find_url(blanked) else {
        return;
    };
    let end = (start..blanked.len())
        .find(|i| blanked[*i].is_whitespace())
        .unwrap_or(blanked.len());
    let url: String = blanked[start..end].iter().collect();
    out.push(finding(
        Check::BareUrl,
        file_path,
        line.number,
        start as u32 + 1,
        format!("bare URL: {}", excerpt(&url, URL_EXCERPT_MAX)),
    ));
}

/// The first `http://` or `https://` that is followed by at least one
/// non-space character, with the length of its scheme.
///
/// The "at least one" is load-bearing: prose that mentions `https://` as a
/// literal string, with nothing after it, is not a URL anyone can follow.
fn find_url(chars: &[char]) -> Option<usize> {
    const SCHEMES: [&[char]; 2] = [
        &['h', 't', 't', 'p', 's', ':', '/', '/'],
        &['h', 't', 't', 'p', ':', '/', '/'],
    ];
    // Both schemes begin with `h`, so one character comparison rejects most
    // positions before any slice compare is set up. Measured over this
    // repository's docs, the guard is 19% of the whole analysis pass: the
    // scan runs over every character of every prose line to find 65 URLs.
    (0..chars.len()).find(|start| {
        chars[*start] == 'h'
            && SCHEMES.iter().any(|scheme| {
                chars[*start..].starts_with(scheme)
                    && chars
                        .get(start + scheme.len())
                        .is_some_and(|c| !c.is_whitespace())
            })
    })
}

/// Brackets or parentheses left unbalanced once the well-formed links are gone.
///
/// Counting over the raw line instead flags every prose parenthetical that
/// wraps onto a second line, which is most of them. The parenthesis half is
/// gated on a surviving `](` for the same reason in the other direction: a
/// sentence like "see the note (below" is bad prose, not broken markdown, and
/// this check has no business having an opinion about it.
fn link_syntax(line: &Line<'_>, blanked: &[char], file_path: &str, out: &mut Vec<Finding>) {
    let count = |target: char| blanked.iter().filter(|c| **c == target).count();
    let (open_square, close_square) = (count('['), count(']'));
    let (open_round, close_round) = (count('('), count(')'));
    // The counts reach the message, not just the comparison. A reader fixing
    // the line needs to know which way it is unbalanced, and a message that
    // depends only on `a != b` is one a miscounting implementation still
    // renders correctly.
    let detail = if open_square != close_square {
        format!("{open_square} `[` against {close_square} `]`")
    } else if blanked.windows(2).any(|w| w == [']', '(']) && open_round != close_round {
        format!("{open_round} `(` against {close_round} `)`")
    } else {
        return;
    };
    out.push(finding(
        Check::LinkSyntaxInvalid,
        file_path,
        line.number,
        1,
        format!("unbalanced link syntax: {detail}"),
    ));
}
