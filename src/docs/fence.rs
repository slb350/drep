//! The single answer to "is this line inside a code fence".
//!
//! Derived once per file and consulted by every check that needs it. The
//! alternative - each check carrying its own `in_fence` toggle through its own
//! loop - is how a `#!/bin/bash` line inside a bash sample got reported as a
//! malformed heading: the heading check's loop had a toggle, and a later
//! refactor moved the check out of the loop that maintained it. If a check in
//! this module's siblings ever tracks fence state itself, that is the bug.

/// A line that opens or closes a fenced code block.
///
/// `trim_start` rather than a bare `starts_with`: an indented fence (inside a
/// list item, say) is still a fence, and CommonMark allows up to three spaces
/// of indentation before one. Four or more makes it an indented code block
/// whose content happens to begin with backticks - a distinction drep does not
/// draw, deliberately, because both readings agree that the line is code.
pub fn is_delimiter(line: &str) -> bool {
    line.trim_start().starts_with("```")
}

/// Fence state for one file: which lines are code, and where the delimiters
/// are.
///
/// Both halves come from the same single pass. The unclosed-fence check needs
/// the delimiter positions and every other fence-aware check needs the mask;
/// deriving them separately would mean two definitions of "delimiter" that
/// could disagree about the same line.
pub struct Fences {
    /// One flag per line, in file order.
    inside: Vec<bool>,
    /// 1-based line numbers of the delimiter lines, in file order.
    delimiters: Vec<u32>,
}

impl Fences {
    /// Scan `lines` once.
    ///
    /// A delimiter line is itself marked as inside a fence. That is not an
    /// off-by-one: ```` ```rust ```` is code punctuation, not prose, so the
    /// prose checks (headings, long lines, links) must not fire on it. A
    /// 130-character ```` ```javascript ```` opener is not a long prose line.
    pub fn scan<S: AsRef<str>>(lines: &[S]) -> Self {
        let mut inside = Vec::with_capacity(lines.len());
        let mut delimiters = Vec::new();
        let mut open = false;
        for (index, line) in lines.iter().enumerate() {
            if is_delimiter(line.as_ref()) {
                // `index + 1` is safe for any file that fits in memory; a
                // 4-billion-line markdown document is not a case worth a
                // fallible signature.
                delimiters.push(index as u32 + 1);
                open = !open;
                inside.push(true);
            } else {
                inside.push(open);
            }
        }
        Self { inside, delimiters }
    }

    /// The per-line flags, in file order, for zipping against the lines.
    pub fn mask(&self) -> &[bool] {
        &self.inside
    }

    /// The 1-based line numbers of the delimiters, in file order.
    pub fn delimiters(&self) -> &[u32] {
        &self.delimiters
    }
}
