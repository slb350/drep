//! Which lines belong to an HTML block.
//!
//! CommonMark 0.31.2, section 4.6: a line that meets one of seven start
//! conditions opens a block of raw HTML that runs to its end condition. Nothing
//! inside it is markdown, so a `#` line in a `<pre>` is not a heading and a URL
//! in a `<div>` is not a bare one. [`mask`] marks those lines for the checks
//! that read markdown (`lines::heading` and the link checks).
//!
//! Types 1 and 3 to 5 end at a marker string and may hold blank lines; types 6
//! and 7 end at the next blank line. Type 2, the comment, is left to
//! `html::block_comment`, which already models it. Three simplifications keep a
//! misreading from hiding a real finding:
//!
//! - **A block with an end marker that never comes is plain text.** Rendered,
//!   it would run to the end of the file, and one stray `<pre` would then
//!   silence every later check. The same rule governs an unclosed comment.
//! - **A block starts only at an indent under four columns**, as CommonMark
//!   requires. Containers are not tracked: a block starting in a list item at
//!   such an indent is still a block, and one in a quote (`> <div>`) is not seen.
//! - **A tag alone on its line (type 7) starts a block only after a blank line,
//!   a fence or another block.** It cannot interrupt a paragraph, and any other
//!   text line, a heading included, is conservatively taken for one.

use crate::docs::html::match_tag;
use crate::docs::indented::indent_columns;

/// The widest indentation a block may start at, in columns.
const MAX_INDENT: usize = 3;

/// Tag names whose blocks run to a closing marker and may hold blank lines
/// (start condition 1). They are also barred from start condition 7.
const RAW_NAMES: [&str; 4] = ["pre", "script", "style", "textarea"];

/// Tag names that start a block ending at a blank line (start condition 6).
const BLOCK_NAMES: [&str; 62] = [
    "address",
    "article",
    "aside",
    "base",
    "basefont",
    "blockquote",
    "body",
    "caption",
    "center",
    "col",
    "colgroup",
    "dd",
    "details",
    "dialog",
    "dir",
    "div",
    "dl",
    "dt",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "frame",
    "frameset",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "head",
    "header",
    "hr",
    "html",
    "iframe",
    "legend",
    "li",
    "link",
    "main",
    "menu",
    "menuitem",
    "nav",
    "noframes",
    "ol",
    "optgroup",
    "option",
    "p",
    "param",
    "search",
    "section",
    "summary",
    "table",
    "tbody",
    "td",
    "tfoot",
    "th",
    "thead",
    "title",
    "tr",
    "track",
    "ul",
];

/// How a block that has started ends.
enum End {
    /// On the first line, the start line included, that holds one of these
    /// strings, whatever their case.
    Marker(&'static [&'static str]),
    /// On the line before the next blank one.
    Blank,
}

/// For each line, whether it sits in an HTML block.
///
/// `fenced` is the code-fence mask: a fenced line starts nothing, and ends the
/// paragraph before it.
pub fn mask<S: AsRef<str>>(lines: &[S], fenced: &[bool]) -> Vec<bool> {
    let mut out = vec![false; lines.len()];
    // Whether the previous line is paragraph text, which type 7 cannot interrupt.
    let mut paragraph = false;
    let mut index = 0;
    while index < lines.len() {
        let text = lines[index].as_ref();
        if fenced[index] || text.trim().is_empty() {
            paragraph = false;
            index += 1;
            continue;
        }
        if let Some(close) = comment_end(lines, index) {
            paragraph = false;
            index = close + 1;
            continue;
        }
        match block_end(lines, index, paragraph) {
            Some(last) => {
                out[index..=last].fill(true);
                paragraph = false;
                index = last + 1;
            }
            None => {
                paragraph = true;
                index += 1;
            }
        }
    }
    out
}

/// The line `text` starts a block on, less its indentation, if the indentation
/// allows one.
fn unindented(text: &str) -> Option<&str> {
    (indent_columns(text) <= MAX_INDENT).then(|| text.trim_start_matches([' ', '\t']))
}

/// The line on which a comment opening at `open` closes, when it does.
///
/// Whatever a closed comment holds is the comment's, so no block starts in it.
fn comment_end<S: AsRef<str>>(lines: &[S], open: usize) -> Option<usize> {
    unindented(lines[open].as_ref())?.strip_prefix("<!--")?;
    // The search includes the opening line itself. Only indentation precedes
    // its `<!--`, so a `-->` there either follows the opener or overlaps it,
    // and the two overlaps, `<!-->` and `<!--->`, are complete comments.
    (open..lines.len()).find(|i| lines[*i].as_ref().contains("-->"))
}

/// The last line of the block that starts at `start`, if one does.
fn block_end<S: AsRef<str>>(lines: &[S], start: usize, paragraph: bool) -> Option<usize> {
    let rest = unindented(lines[start].as_ref())?;
    match kind(rest, paragraph)? {
        End::Marker(markers) => (start..lines.len()).find(|i| {
            let line = lines[*i].as_ref().as_bytes();
            markers.iter().any(|marker| {
                line.windows(marker.len())
                    .any(|window| window.eq_ignore_ascii_case(marker.as_bytes()))
            })
        }),
        // `mask` never starts a block on a blank line, so the search may
        // include `start`.
        End::Blank => Some(
            (start..lines.len())
                .find(|i| lines[*i].as_ref().trim().is_empty())
                .unwrap_or(lines.len())
                - 1,
        ),
    }
}

/// Which start condition `rest`, a line without its indentation, meets.
fn kind(rest: &str, paragraph: bool) -> Option<End> {
    let after = rest.strip_prefix('<')?;
    if let Some(declaration) = after.strip_prefix('!') {
        return if declaration.starts_with("[CDATA[") {
            Some(End::Marker(&["]]>"]))
        } else if declaration.starts_with(|c: char| c.is_ascii_alphabetic()) {
            Some(End::Marker(&[">"]))
        } else {
            None
        };
    }
    if after.starts_with('?') {
        return Some(End::Marker(&["?>"]));
    }
    if named(after, &RAW_NAMES, &[" ", "\t", ">"]) {
        return Some(End::Marker(&[
            "</pre>",
            "</script>",
            "</style>",
            "</textarea>",
        ]));
    }
    let name = after.strip_prefix('/').unwrap_or(after);
    if named(name, &BLOCK_NAMES, &[" ", "\t", ">", "/>"]) {
        return Some(End::Blank);
    }
    (!paragraph && is_tag_alone(rest)).then_some(End::Blank)
}

/// Whether `text` starts with a tag name from sorted ASCII slice `names`, then
/// the end of the line or one of `followers`.
fn named(text: &str, names: &[&str], followers: &[&str]) -> bool {
    let end = text
        .bytes()
        .position(|b| !b.is_ascii_alphanumeric())
        .unwrap_or(text.len());
    names
        .binary_search_by(|name| cmp_ascii_case(name, &text[..end]))
        .is_ok()
        && (end == text.len() || followers.iter().any(|f| text[end..].starts_with(f)))
}

fn cmp_ascii_case(a: &str, b: &str) -> std::cmp::Ordering {
    a.bytes()
        .map(|byte| byte.to_ascii_lowercase())
        .cmp(b.bytes().map(|byte| byte.to_ascii_lowercase()))
}

/// Whether `rest` is one complete open or closing tag, then spaces and tabs
/// only (start condition 7).
fn is_tag_alone(rest: &str) -> bool {
    let name = rest[1..].strip_prefix('/').unwrap_or(&rest[1..]);
    let closing = name.len() != rest.len() - 1;
    let end = name
        .bytes()
        .position(|b| !(b.is_ascii_alphanumeric() || b == b'-'))
        .unwrap_or(name.len());
    if !closing
        && RAW_NAMES
            .iter()
            .any(|raw| name[..end].eq_ignore_ascii_case(raw))
    {
        return false;
    }
    let chars: Vec<char> = rest.chars().collect();
    match_tag(&chars, 0)
        .is_some_and(|close| chars[close + 1..].iter().all(|c| matches!(c, ' ' | '\t')))
}
