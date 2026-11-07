//! Which lines belong to an indented code block.
//!
//! CommonMark 0.31.2, section 4.4: a chunk of lines indented four columns or
//! more is code, but only where it begins a block. The checks that ask whether
//! a line is code (see `lines`) cannot tell a fenced sample from an indented
//! one, so [`mask`] folds both into the one flag every check already consults.
//!
//! Two situations are read as prose on purpose, because reading either as code
//! would silence findings on text a reader sees as prose:
//!
//! - **A paragraph continuation.** An indented line cannot interrupt a
//!   paragraph, so it must follow a blank line, another code line, the top of
//!   the file, a fence, an ATX heading or a thematic break. The last two are
//!   leaf blocks that no paragraph continues past, so no blank line is needed.
//! - **Under a list item.** The item's content offset decides whether four
//!   columns is code, and drep does not track it, so an indented line is
//!   prose from a list marker until a blank line is followed by an unindented
//!   line that is not a marker. Missing a block there costs a possible false
//!   positive; guessing wrong would hide a real finding.

/// Columns of indentation that make a line code.
const CODE_INDENT: usize = 4;

/// What the previous line was, as far as the next one's classification cares.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Previous {
    /// No line yet.
    Start,
    Blank,
    /// An indented code line.
    Code,
    /// A fence delimiter or a line inside a fence.
    Fence,
    /// An ATX heading or a thematic break: a one-line block that ends the
    /// paragraph before it and that no later line continues.
    Leaf,
    /// Anything else: a paragraph, a list item, a quote.
    Text,
}

impl Previous {
    /// Whether a block may begin on the line after this one.
    fn permits_code(self) -> bool {
        self != Previous::Text
    }
}

/// The code mask for `lines`: true for a line inside a fence, as given by
/// `fenced`, and for every line of an indented code block.
///
/// A blank line is marked when it is inside a fence, or when code lines sit on
/// both sides of it: it is then content of the block, and a run of them is not
/// a run of blank lines in prose.
pub fn mask<S: AsRef<str>>(lines: &[S], fenced: &[bool]) -> Vec<bool> {
    let mut out = Vec::with_capacity(lines.len());
    let mut previous = Previous::Start;
    let mut in_list = false;
    for (line, fenced) in lines.iter().zip(fenced) {
        let line = line.as_ref();
        if line.trim().is_empty() {
            out.push(*fenced);
            previous = Previous::Blank;
            continue;
        }
        let indent = indent_columns(line);
        // Inside a fence a `#` line is a comment and `---` is content.
        let leaf = !*fenced && indent < CODE_INDENT && is_leaf(line);
        // A fence delimiter is a line like any other as far as a list is
        // concerned, so the list bookkeeping runs before the fence test.
        if indent < CODE_INDENT {
            if leaf {
                // Without indentation it is not item content, and neither a
                // heading nor a break can be a lazy continuation line.
                in_list &= indent != 0;
            } else if starts_list_item(line) {
                in_list = true;
            } else if indent == 0 && previous == Previous::Blank {
                in_list = false;
            }
        }
        if *fenced {
            out.push(true);
            previous = Previous::Fence;
        } else if leaf {
            out.push(false);
            previous = Previous::Leaf;
        } else if indent >= CODE_INDENT && previous.permits_code() && !in_list {
            out.push(true);
            previous = Previous::Code;
        } else {
            out.push(false);
            previous = Previous::Text;
        }
    }
    mark_blanks_between_code(lines, fenced, &mut out);
    out
}

/// Mark each run of blank lines that has an indented code line on both sides.
fn mark_blanks_between_code<S: AsRef<str>>(lines: &[S], fenced: &[bool], out: &mut [bool]) {
    let mut start = 0;
    while start < lines.len() {
        if out[start] || !lines[start].as_ref().trim().is_empty() {
            start += 1;
            continue;
        }
        let end = (start..lines.len())
            .find(|i| !lines[*i].as_ref().trim().is_empty())
            .unwrap_or(lines.len());
        let indented = |i: usize| out[i] && !fenced[i];
        if start > 0 && end < lines.len() && indented(start - 1) && indented(end) {
            out[start..end].fill(true);
        }
        start = end;
    }
}

/// The width of the leading whitespace, counting a tab as advancing to the
/// next multiple of four columns (CommonMark 0.31.2, section 2.2).
///
/// Visible to the docs tests because no document can observe the exact width:
/// see `indent_columns_expands_a_tab_to_the_next_stop`.
pub(super) fn indent_columns(line: &str) -> usize {
    let mut columns = 0;
    for byte in line.bytes() {
        match byte {
            b' ' => columns += 1,
            b'\t' => columns += CODE_INDENT - columns % CODE_INDENT,
            _ => break,
        }
    }
    columns
}

/// Whether the line, less up to three columns of indentation, is an ATX heading
/// or a thematic break (CommonMark 0.31.2, sections 4.1 and 4.2).
///
/// A heading is one to six `#` followed by whitespace or the line's end, so
/// `#hashtag` and seven markers are paragraph text. A break is three or more
/// of one of `-`, `*` and `_`, with spaces and tabs allowed between them. It is
/// tested ahead of the list marker because `- - -` is a break, not an item.
/// A setext underline needs no case of its own: `---` is a break either way,
/// and `===` is left as text.
///
/// Called only for an indent under four, so trimming all leading whitespace
/// removes exactly the indentation.
fn is_leaf(line: &str) -> bool {
    let rest = line.trim_start();
    let markers = rest.bytes().take_while(|b| *b == b'#').count();
    if (1..=6).contains(&markers) {
        return rest[markers..].is_empty() || rest[markers..].starts_with([' ', '\t']);
    }
    let Some(mark) = rest
        .bytes()
        .next()
        .filter(|b| matches!(b, b'-' | b'*' | b'_'))
    else {
        return false;
    };
    let mut count = 0;
    for byte in rest.bytes() {
        if byte == mark {
            count += 1;
        } else if byte != b' ' && byte != b'\t' {
            return false;
        }
    }
    count >= 3
}

/// Whether the line, less up to three columns of indentation, opens with a
/// bullet or an ordered-list marker followed by whitespace or the line's end.
///
/// Called only for an indent under four, so trimming all leading whitespace
/// removes exactly the indentation.
fn starts_list_item(line: &str) -> bool {
    let rest = line.trim_start();
    let after = if let Some(after) = rest.strip_prefix(['-', '*', '+']) {
        after
    } else {
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        // CommonMark allows nine digits at most.
        if !(1..=9).contains(&digits) {
            return false;
        }
        match rest[digits..].strip_prefix(['.', ')']) {
            Some(after) => after,
            None => return false,
        }
    };
    after.is_empty() || after.starts_with([' ', '\t'])
}
