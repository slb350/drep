//! Raw inline HTML, which is not markdown.
//!
//! A URL in an attribute (`<img src="https://...">`) is the markup's own
//! address: it is not prose, and wrapping it in `[text](url)` would break the
//! tag. The link checks therefore blank every tag and comment before they look
//! at a line, and this module decides which columns those are.
//!
//! It decides over a whole paragraph, because CommonMark lets a tag's parts
//! be separated by a line ending, a quoted value hold one, and a comment run
//! over several lines. A tag is recognised by the grammar of the spec's section
//! 6.6 rather than by "from `<` to `>`", because a looser rule would swallow
//! prose: `x<y https://example.com>` is not a tag, since a URL is not an
//! attribute name, and the URL in it is as bare as any other. Only the tag is
//! blanked; text between two tags is markdown, so a bare URL there still counts.
//!
//! Two scopes are searched:
//!
//! - a *paragraph* - consecutive lines outside any code fence, ended by a blank
//!   line - where a tag or comment may begin anywhere;
//! - a *block comment* - `<!--` opening a line with at most three spaces of
//!   indentation - which, as an HTML block, also runs over blank lines until
//!   the line holding `-->`.
//!
//! A tag or comment that is never closed is plain text, as before. Code spans
//! are blanked first, as in the link checks, and are likewise recognised over a
//! line ending here, so a backticked `<!--` opens nothing. Columns are
//! character offsets into the line, matching what the checks report.

use std::ops::Range;

use crate::docs::Line;
use crate::docs::links::blank_inline_code;

/// For each line, the character ranges that raw HTML covers.
///
/// A line outside any tag or comment has an empty list, which does not
/// allocate.
pub(super) fn spans(lines: &[Line<'_>]) -> Vec<Vec<Range<usize>>> {
    let mut spans = vec![Vec::new(); lines.len()];
    let mut index = 0;
    while index < lines.len() {
        // Resume after the comment: what it holds is already blanked.
        if let Some(close) = block_comment(lines, index, &mut spans) {
            index = close;
        }
        index += 1;
    }
    let mut joined = Vec::new();
    let mut start = 0;
    while start < lines.len() {
        if !is_prose(&lines[start]) {
            start += 1;
            continue;
        }
        let end = (start..lines.len())
            .find(|i| !is_prose(&lines[*i]))
            .unwrap_or(lines.len());
        paragraph(&lines[start..end], &mut spans[start..end], &mut joined);
        start = end;
    }
    spans
}

/// Whether `line` belongs to a paragraph: outside a fence and not blank.
fn is_prose(line: &Line<'_>) -> bool {
    !line.in_fence && !line.text.trim().is_empty()
}

/// Record the tags and comments in one paragraph.
///
/// `joined` is a buffer reused across paragraphs: the lines' characters with a
/// `\n` between neighbours.
fn paragraph(lines: &[Line<'_>], spans: &mut [Vec<Range<usize>>], joined: &mut Vec<char>) {
    joined.clear();
    let mut extents = Vec::with_capacity(lines.len());
    for line in lines {
        let start = joined.len();
        joined.extend(line.text.chars());
        extents.push(start..joined.len());
        joined.push('\n');
    }
    blank_inline_code(joined);
    let mut i = 0;
    while i < joined.len() {
        if joined[i] == '<'
            && let Some(end) = match_comment(joined, i).or_else(|| match_tag(joined, i))
        {
            record(joined, &extents, i..end + 1, spans);
            i = end;
        }
        i += 1;
    }
}

/// Split `found`, a range of the joined paragraph, into the per-line ranges
/// it covers. The line ending between two lines belongs to neither.
///
/// `extents` holds where each line sits in the joined text.
fn record(
    joined: &[char],
    extents: &[Range<usize>],
    found: Range<usize>,
    spans: &mut [Vec<Range<usize>>],
) {
    let first = extents.partition_point(|extent| extent.start <= found.start) - 1;
    let covered = joined[found.clone()].iter().filter(|c| **c == '\n').count() + 1;
    for (extent, spans) in extents[first..first + covered]
        .iter()
        .zip(&mut spans[first..first + covered])
    {
        let lo = found.start.saturating_sub(extent.start);
        let hi = found.end.min(extent.end) - extent.start;
        spans.push(lo..hi);
    }
}

/// Record a comment that opens a line and closes on a later one, returning
/// the index of the closing line.
///
/// A comment closed on its own line is left to the paragraph scan, which
/// treats it exactly like one in the middle of a sentence, and so is a comment
/// with four or more spaces before it: indented code is not modelled, so that
/// opener is ordinary text.
fn block_comment(
    lines: &[Line<'_>],
    open: usize,
    spans: &mut [Vec<Range<usize>>],
) -> Option<usize> {
    let line = &lines[open];
    if line.in_fence {
        return None;
    }
    let chars: Vec<char> = line.text.chars().collect();
    let indent = chars.iter().take_while(|c| **c == ' ').count();
    if indent > 3 || match_comment(&chars, indent).is_some() {
        return None;
    }
    // Spaces are one byte and one character, so the count indexes both.
    if !line.text[indent..].starts_with("<!--") {
        return None;
    }
    let (close, after) = (open + 1..lines.len()).find_map(|index| {
        let text = lines[index].text;
        text.find("-->")
            .map(|byte| (index, text[..byte].chars().count() + "-->".len()))
    })?;
    // The opener's leading spaces are blanked with it, which no check can see.
    for index in open..close {
        spans[index].push(0..width(&lines[index]));
    }
    spans[close].push(0..after);
    Some(close)
}

/// A line's length in characters.
fn width(line: &Line<'_>) -> usize {
    line.text.chars().count()
}

/// Match `<!-- ... -->` starting at `start`, returning the index of its last `>`.
///
/// The search for `-->` begins inside the opener, as CommonMark 0.31.2 defines
/// it: `<!-->` and `<!--->` are complete comments.
fn match_comment(chars: &[char], start: usize) -> Option<usize> {
    const OPEN: [char; 4] = ['<', '!', '-', '-'];
    const CLOSE: [char; 3] = ['-', '-', '>'];
    if !chars[start..].starts_with(&OPEN) {
        return None;
    }
    (start + 2..chars.len())
        .find(|i| chars[*i..].starts_with(&CLOSE))
        .map(|i| i + CLOSE.len() - 1)
}

/// Match an open or closing tag starting at `start`, returning the index of its
/// `>`.
fn match_tag(chars: &[char], start: usize) -> Option<usize> {
    debug_assert_eq!(chars[start], '<');
    let closing = chars.get(start + 1) == Some(&'/');
    let name = start + 1 + usize::from(closing);
    if !chars.get(name)?.is_ascii_alphabetic() {
        return None;
    }
    // From `name`, not `name + 1`: its first letter already satisfies the
    // predicate, so skipping it by hand would be an unobservable difference.
    let mut i = skip_while(chars, name, |c| c.is_ascii_alphanumeric() || c == '-');
    if closing {
        i = skip_while(chars, i, is_tag_space);
        return (chars.get(i) == Some(&'>')).then_some(i);
    }
    loop {
        let attributes = skip_while(chars, i, is_tag_space);
        // An attribute needs whitespace before it, so `<a href="x"href="y">` is
        // not a tag.
        let separated = attributes > i;
        match *chars.get(attributes)? {
            '>' => return Some(attributes),
            // One index both tested and returned, so the `>` it names is the
            // one that was checked.
            '/' => {
                let close = attributes + 1;
                return (chars.get(close) == Some(&'>')).then_some(close);
            }
            c if separated && (c.is_ascii_alphabetic() || matches!(c, '_' | ':')) => {
                i = attribute_end(chars, attributes)?;
            }
            _ => return None,
        }
    }
}

/// The index just past the attribute whose name starts at `start`, with its
/// value if it has one.
fn attribute_end(chars: &[char], start: usize) -> Option<usize> {
    let name_end = skip_while(chars, start, |c| {
        c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | ':' | '-')
    });
    let equals = skip_while(chars, name_end, is_tag_space);
    if chars.get(equals) != Some(&'=') {
        return Some(name_end);
    }
    let value = skip_while(chars, equals + 1, is_tag_space);
    match *chars.get(value)? {
        quote @ ('"' | '\'') => (value + 1..chars.len())
            .find(|i| chars[*i] == quote)
            .map(|close| close + 1),
        _ => {
            let end = skip_while(chars, value, |c| {
                !is_tag_space(c) && !matches!(c, '"' | '\'' | '=' | '<' | '>' | '`')
            });
            (end > value).then_some(end)
        }
    }
}

/// Whitespace as the raw HTML grammar counts it inside a tag: spaces, tabs and
/// line endings. A no-break space is whitespace to Rust but text to CommonMark.
fn is_tag_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n')
}

/// The first index at or after `from` whose character fails `keep`.
fn skip_while(chars: &[char], from: usize, keep: impl Fn(char) -> bool) -> usize {
    (from..chars.len())
        .find(|i| !keep(chars[*i]))
        .unwrap_or(chars.len())
}
