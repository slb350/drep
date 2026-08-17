//! The text the LLM sees, plus the line numbers that text legitimately
//! covers.
//!
//! The format exists to solve one problem: **line-number provenance**. If
//! the model is handed a bare diff it must infer file line numbers from the
//! `@@` header, which it does unreliably; every finding then points at the
//! wrong code and looks perfectly plausible. So the payload states each
//! line's real file line number explicitly in the gutter, and the caller
//! keeps the set of numbers that were actually shown. A later phase drops
//! any finding whose line is not in that set, because such a finding is
//! about code the model was never shown.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::Path;

use crate::diff::hunks::{Hunk, HunkLine};

/// A rendered payload plus the file line numbers it legitimately covers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Payload {
    /// The text handed to the model.
    pub text: String,
    /// Every new-file line number that appears in `text` with a number in the
    /// gutter — Context and Added lines. A later phase drops any finding whose
    /// line is not in this set, because such a finding is about code the model
    /// was never shown.
    pub valid_lines: BTreeSet<u32>,
}

/// Render every hunk belonging to one file into a single payload.
///
/// `hunks` must all share `file_path`; they are rendered in ascending
/// `new_start` order. Returns `None` when `hunks` is empty.
pub fn render(file_path: &Path, language_name: &str, hunks: &[Hunk]) -> Option<Payload> {
    if hunks.is_empty() {
        return None;
    }

    let mut sorted: Vec<&Hunk> = hunks.iter().collect();
    sorted.sort_by_key(|h| h.new_start);

    let (text, valid_lines) = build_payload(file_path, language_name, &sorted);
    Some(Payload { text, valid_lines })
}

/// Build the payload text and the `valid_lines` set in one walk.
///
/// Splitting the helper out makes it possible to test the set construction
/// without re-asserting the full text, and keeps `render` a single entry
/// point. The set is built as it walks the lines: every Context line
/// contributes its new-file number, every Added line does the same, and
/// Removed lines contribute nothing — the gutter for a Removed line is six
/// spaces, deliberately, so no number ever appears for one.
fn build_payload(
    file_path: &Path,
    language_name: &str,
    hunks: &[&Hunk],
) -> (String, BTreeSet<u32>) {
    let mut text = String::new();
    let mut valid_lines: BTreeSet<u32> = BTreeSet::new();

    // `write!` into the buffer rather than `push_str(&format!(..))`: the latter
    // allocates a throwaway `String` for every rendered line, and a payload is
    // routinely well over a thousand lines. Writing to a `String` cannot fail,
    // so the `Result` is discarded deliberately.
    let _ = writeln!(text, "File: {}", file_path.display());
    let _ = writeln!(text, "Language: {language_name}");
    text.push('\n');
    text.push_str(scope_sentence(hunks));
    text.push('\n');
    text.push('\n');

    let mut last_numbered: Option<u32> = None;

    for hunk in hunks {
        if let Some(prev) = last_numbered {
            let gap = hunk.new_start.saturating_sub(prev.saturating_add(1));
            if gap > 0 {
                let _ = writeln!(text, "... {gap} lines omitted ...");
            }
        }

        let mut current_number = hunk.new_start;
        for line in &hunk.lines {
            // The marker is bound by the pattern rather than recovered with a
            // second `matches!` on an already-destructured value.
            let (marker, content) = match line {
                HunkLine::Context(content) => (' ', content),
                HunkLine::Added(content) => ('+', content),
                HunkLine::Removed(content) => {
                    // Six spaces where the number goes: a removed line has no
                    // line in the new file, and inventing one is the error
                    // this whole format exists to prevent. `current_number`
                    // and `valid_lines` are both left untouched.
                    let _ = writeln!(text, "-{:>6} | {content}", "");
                    continue;
                }
            };
            let _ = writeln!(text, "{marker}{current_number:>6} | {content}");
            valid_lines.insert(current_number);
            last_numbered = Some(current_number);
            current_number = current_number.saturating_add(1);
        }
    }

    (text, valid_lines)
}

/// Pick the right scope sentence for this set of hunks.
///
/// Whole-file mode (every line is `Context`) tells the model to review the
/// whole file; diff mode (some line is `Added` or `Removed`) tells it to
/// focus on the marked lines and never report findings on removed lines.
/// The decision reads from the data so the caller cannot get a flag wrong.
fn scope_sentence(hunks: &[&Hunk]) -> &'static str {
    let has_change = hunks
        .iter()
        .any(|h| h.lines.iter().any(|l| !matches!(l, HunkLine::Context(_))));
    if has_change {
        "Review the lines marked `+`. Lines with no marker are unchanged context. \
         Lines marked `-` were removed and have no line number; do not report \
         findings on them. Report each finding using the line number shown in the gutter."
    } else {
        "Review the entire file. Report each finding using the line number shown in the gutter."
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::diff::hunks::Hunk;

    #[test]
    fn empty_hunk_slice_returns_none() {
        let hunk = Hunk {
            file_path: std::path::PathBuf::from("x.rs"),
            old_start: 0,
            old_count: 0,
            new_start: 1,
            new_count: 1,
            lines: vec![HunkLine::Context("only".to_owned())],
        };
        assert!(render(Path::new("x.rs"), "rust", std::slice::from_ref(&hunk)).is_some());
        assert!(render(Path::new("x.rs"), "rust", &[]).is_none());
    }
}
