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
//!   the file or a fence.
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
    /// Anything else: a paragraph, a heading, a list item.
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
        // A fence delimiter is a line like any other as far as a list is
        // concerned, so the list bookkeeping runs before the fence test.
        if indent < CODE_INDENT {
            if starts_list_item(line) {
                in_list = true;
            } else if indent == 0 && previous == Previous::Blank {
                in_list = false;
            }
        }
        if *fenced {
            out.push(true);
            previous = Previous::Fence;
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
/// next multiple of four columns.
fn indent_columns(line: &str) -> usize {
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
