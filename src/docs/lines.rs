//! Checks that need one line and nothing else.
//!
//! Five of the ten. Three consult the fence mask and two deliberately do not;
//! the rule deciding which is in [`check`]'s doc comment, because it is the
//! kind of asymmetry that looks like an oversight until it is written down.

use crate::analysis::findings::Finding;
use crate::docs::{Check, LONG_LINE_MAX, Line, finding};

/// Run the per-line checks over `line`, pushing onto `out`.
///
/// **Which checks respect a code fence**: the ones whose advice would be wrong
/// inside one.
///
/// - Headings, long lines: prose rules. `#!/bin/bash` inside a ```` ```bash ````
///   sample is a shebang, not a malformed heading - the specific false positive
///   the fence mask was built for - and a 200-character line of code is not a
///   paragraph that needs wrapping.
/// - Tabs: "replace tabs with spaces" is *wrong* inside a fence. A ```` ```make ````
///   sample whose recipe lines are indented with tabs stops being a Makefile if
///   you follow the advice. 1.x flagged tabs everywhere; this is a deliberate
///   divergence, on the grounds that a check which cannot give correct advice
///   about a line should not fire on it.
/// - Trailing whitespace: fires everywhere, because its advice is right
///   everywhere. Whitespace before a newline is junk in prose and junk in a
///   code sample alike, and unlike a tab it carries no meaning in either.
pub fn check(line: &Line<'_>, chars: &[char], file_path: &str, out: &mut Vec<Finding>) {
    trailing_whitespace(line, chars, file_path, out);
    if !line.in_fence {
        tab_character(line, chars, file_path, out);
        heading(line, chars, file_path, out);
        long_line(line, chars, file_path, out);
    }
}

/// Spaces or tabs immediately before the line ending.
///
/// The column is the first character of the trailing run, so a fix knows where
/// to cut. Only spaces and tabs count: a trailing non-breaking space is a
/// character the author typed on purpose (or a bug of a different kind), and
/// silently deleting it is not this check's call.
fn trailing_whitespace(line: &Line<'_>, chars: &[char], file_path: &str, out: &mut Vec<Finding>) {
    let kept = chars
        .iter()
        .rposition(|c| *c != ' ' && *c != '\t')
        .map_or(0, |last| last + 1);
    if kept == chars.len() {
        return;
    }
    out.push(finding(
        Check::TrailingWhitespace,
        file_path,
        line.number,
        kept as u32 + 1,
        format!("{} trailing whitespace character(s)", chars.len() - kept),
    ));
}

/// A literal tab anywhere in the line. See [`check`] for why fences are exempt.
fn tab_character(line: &Line<'_>, chars: &[char], file_path: &str, out: &mut Vec<Finding>) {
    let Some(index) = chars.iter().position(|c| *c == '\t') else {
        return;
    };
    out.push(finding(
        Check::TabCharacter,
        file_path,
        line.number,
        index as u32 + 1,
        "tab character".to_owned(),
    ));
}

/// Markdown allows at most six `#`s in an ATX heading.
const MAX_HEADING_LEVEL: usize = 6;

/// The two heading defects: a heading with no text, and one with no space
/// after its marker.
///
/// Mutually exclusive by construction - the first needs everything after the
/// `#`s to be blank and the second needs the very next character not to be -
/// but written as one function because they share the level count, and a level
/// outside 1..=6 disqualifies both. `#######text` is not a heading at all in
/// markdown, so drep has nothing to say about it.
fn heading(line: &Line<'_>, chars: &[char], file_path: &str, out: &mut Vec<Finding>) {
    if !line.text.starts_with('#') {
        return;
    }
    let level = chars.iter().take_while(|c| **c == '#').count();
    if level > MAX_HEADING_LEVEL {
        return;
    }
    let rest = &chars[level..];
    if rest.iter().all(|c| c.is_whitespace()) {
        out.push(finding(
            Check::EmptyHeading,
            file_path,
            line.number,
            1,
            format!("level-{level} heading has no text"),
        ));
    } else if !rest[0].is_whitespace() {
        // `rest[0]` cannot be `#`: the `take_while` above consumed every
        // leading one, so the first survivor is something else.
        out.push(finding(
            Check::MissingSpaceAfterHeading,
            file_path,
            line.number,
            level as u32 + 1,
            format!("no space after the {level} heading marker(s)"),
        ));
    }
}

/// A prose line longer than [`LONG_LINE_MAX`] characters.
///
/// Characters, not bytes: a line of prose does not become too long because it
/// contains an em dash. The column is the first character past the limit.
fn long_line(line: &Line<'_>, chars: &[char], file_path: &str, out: &mut Vec<Finding>) {
    let length = chars.len();
    if length <= LONG_LINE_MAX {
        return;
    }
    out.push(finding(
        Check::LongLine,
        file_path,
        line.number,
        LONG_LINE_MAX as u32 + 1,
        format!("line is {length} characters (limit {LONG_LINE_MAX})"),
    ));
}
