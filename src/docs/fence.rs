//! The single answer to "is this line inside a code fence".
//!
//! Derived once per file and consulted by every check that needs it. The
//! alternative - each check carrying its own `in_fence` toggle through its own
//! loop - is how a `#!/bin/bash` line inside a bash sample got reported as a
//! malformed heading: the heading check's loop had a toggle, and a later
//! refactor moved the check out of the loop that maintained it. If a check in
//! this module's siblings ever tracks fence state itself, that is the bug.
//!
//! What counts as a fence is CommonMark 0.31.2's fenced code block (section
//! 4.5), because the checks' severities answer whether the document *renders*
//! differently and that is the renderer's rule. A toggle on any line starting
//! with three backticks got four documents wrong, one of which was an `error`
//! that blocks a commit by default: a `~~~` fence was never a fence, a
//! four-backtick fence that documents a three-backtick one closed early, a
//! `~~~` sample holding a ```` ``` ```` line read as unclosed, and a line with an
//! info string closed the block it sat in.

/// A run of at least three backticks or tildes opening a line, and what follows it.
struct Marker<'a> {
    /// `` ` `` or `~`. Tildes and backticks never mix within one fence.
    character: char,
    /// How many of them, so a closing fence can be required to be as long.
    length: usize,
    /// Everything after the run: an info string on an opener, nothing on a closer.
    rest: &'a str,
}

impl Marker<'_> {
    /// Whether this line starts a fenced block.
    ///
    /// A backtick fence's info string may not contain a backtick, because
    /// ```` ``` aa ``` ```` is an inline code span in a paragraph. A tilde fence's
    /// may.
    fn opens(&self) -> bool {
        self.character == '~' || !self.rest.contains('`')
    }

    /// Whether this line closes a block opened by `length` of `character`.
    ///
    /// The same character, at least as many of it, and nothing after it but
    /// whitespace: a line with an info string is content of the block it sits
    /// in, which is how a markdown sample shows an opening fence.
    fn closes(&self, character: char, length: usize) -> bool {
        self.character == character && self.length >= length && self.rest.trim().is_empty()
    }
}

/// The fence marker `line` begins with, if it begins with one.
///
/// Any amount of indentation is accepted where CommonMark allows three spaces.
/// The relaxation is deliberate: a fence inside a list item sits at the item's
/// content indentation, which drep does not track, and reading it as prose would
/// put the whole sample back into every prose check. A four-space-indented marker
/// that CommonMark reads as an indented code block is code on both readings.
fn marker(line: &str) -> Option<Marker<'_>> {
    let line = line.trim_start();
    let character = line.chars().next().filter(|c| matches!(c, '`' | '~'))?;
    // Both characters are ASCII, so the run's length in characters is its length in bytes.
    let length = line.chars().take_while(|c| *c == character).count();
    (length >= 3).then(|| Marker {
        character,
        length,
        rest: &line[length..],
    })
}

/// A block that has been opened and not yet closed.
struct Open {
    character: char,
    length: usize,
    /// 1-based line number of its opening fence.
    line: u32,
}

/// Fence state for one file: which lines are code, and which block, if any, is
/// never closed.
///
/// Both halves come from the same single pass. The unclosed-fence check needs
/// the opener's position and every other fence-aware check needs the mask;
/// deriving them separately would mean two definitions of "fence" that could
/// disagree about the same line.
pub struct Fences {
    /// One flag per line, in file order.
    inside: Vec<bool>,
    /// 1-based line number of the opening fence of the block still open at the
    /// end of the file.
    unclosed: Option<u32>,
}

impl Fences {
    /// Scan `lines` once.
    ///
    /// A fence line, opening or closing, is itself marked as inside. That is
    /// not an off-by-one: ```` ```rust ```` is code punctuation, not prose, so
    /// the prose checks (headings, long lines, links) must not fire on it. A
    /// 130-character ```` ```javascript ```` opener is not a long prose line.
    ///
    /// A block with no closing fence runs to the end of the file, as it does
    /// when rendered; its opener is what [`Fences::unclosed`] returns.
    pub fn scan<S: AsRef<str>>(lines: &[S]) -> Self {
        let mut inside = Vec::with_capacity(lines.len());
        let mut open: Option<Open> = None;
        for (index, line) in lines.iter().enumerate() {
            let found = marker(line.as_ref());
            match (&open, found) {
                (None, Some(marker)) if marker.opens() => {
                    open = Some(Open {
                        character: marker.character,
                        length: marker.length,
                        // `index + 1` is safe for any file that fits in memory; a
                        // 4-billion-line markdown document is not a case worth a
                        // fallible signature.
                        line: index as u32 + 1,
                    });
                    inside.push(true);
                }
                (Some(block), Some(marker)) if marker.closes(block.character, block.length) => {
                    open = None;
                    inside.push(true);
                }
                _ => inside.push(open.is_some()),
            }
        }
        Self {
            inside,
            unclosed: open.map(|block| block.line),
        }
    }

    /// The per-line flags, in file order, for zipping against the lines.
    pub fn mask(&self) -> &[bool] {
        &self.inside
    }

    /// The 1-based line number of the opening fence of the block the file never
    /// closes, if there is one.
    pub fn unclosed(&self) -> Option<u32> {
        self.unclosed
    }
}
