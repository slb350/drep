//! The text the LLM sees, plus the line numbers that text legitimately
//! covers.
//!
//! The format exists to solve one problem: **line-number provenance**. If the
//! model is handed a bare diff it must infer file line numbers from the `@@`
//! header, which it does unreliably; every finding then points at the wrong
//! code and looks perfectly plausible. So the payload states each line's real
//! file line number explicitly in the gutter, and the caller keeps the set of
//! numbers that were actually shown. A later phase drops any finding whose
//! line is not in that set, because such a finding is about code the model was
//! never shown.
//!
//! The numbering itself is not implemented here: it comes from
//! [`Hunk::numbered_lines`], which is the single home of the rule that a
//! removed line does not consume a line number.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use crate::diff::hunks::Hunk;
use crate::languages::spec::LanguageSupport;

/// The largest payload drep will send to the model, in bytes.
///
/// Enforced **here**, on the rendered text, rather than on one branch of input
/// resolution. It used to live in `cli::check` and was consulted only in paths
/// mode, so a newly-added 5 MB file reached the LLM whole through `--staged`
/// or `--diff` - the two modes a commit gate actually runs in. The ceiling
/// belongs where the payload is built, because that is the one place every
/// input mode passes through.
///
/// A payload over the ceiling is a
/// [`crate::analysis::result::FailureReason::PayloadTooLarge`] failure, never a
/// skip: 1.x returned an empty finding list for anything over 32k chars, which
/// under this codebase's contract is the banned move - a file drep declined to
/// analyze is not clean.
///
/// `u64` rather than `usize` so it compares directly against the byte counts
/// `FailureReason` carries; only one site measures a `str` length, and that one
/// casts.
pub const PAYLOAD_MAX_BYTES: u64 = 256 * 1024;

/// A rendered payload plus the file line numbers it legitimately covers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Payload {
    /// The text handed to the model.
    pub text: String,
    /// Every new-file line number that appears in `text` with a number in the
    /// gutter — `Context` and `Added` lines. A later phase drops any finding
    /// whose line is not in this set, because such a finding is about code the
    /// model was never shown.
    ///
    /// Context lines are included deliberately: they were shown with their
    /// real numbers, so a finding on one is an observation about code the
    /// model actually read, not a hallucination.
    pub valid_lines: BTreeSet<u32>,
}

/// Render every hunk belonging to one file into a single payload.
///
/// The file path is taken from the hunks themselves rather than passed
/// alongside them — they all carry it, and a separate argument would be a
/// second copy for the caller to keep in sync with no way to check it.
/// `hunks` must therefore all share a `file_path`; they are rendered in
/// ascending `new_start` order. Returns `None` when `hunks` is empty.
///
/// The language arrives as a [`LanguageSupport`], not a string: `languages/`
/// is the only place a language is named, and `display_name` is the name
/// meant for the model. A bare `&str` here would let a caller pass the
/// registry key (`"rust"`) where the prompt wants `"Rust"`, with nothing to
/// catch it.
pub fn render(language: &LanguageSupport, hunks: &[Hunk]) -> Option<Payload> {
    let file_path = &hunks.first()?.file_path;

    let mut sorted: Vec<&Hunk> = hunks.iter().collect();
    sorted.sort_by_key(|h| h.new_start);

    let mut text = String::new();
    let mut valid_lines: BTreeSet<u32> = BTreeSet::new();

    // `write!` into the buffer rather than `push_str(&format!(..))`: the latter
    // allocates a throwaway `String` for every rendered line, and a payload is
    // routinely well over a thousand lines. Writing to a `String` cannot fail,
    // so the `Result` is discarded deliberately.
    let _ = writeln!(text, "File: {}", file_path.display());
    let _ = writeln!(text, "Language: {}", language.display_name);
    text.push('\n');
    text.push_str(scope_sentence(&sorted));
    text.push('\n');
    text.push('\n');

    let mut last_numbered: Option<u32> = None;

    for hunk in &sorted {
        if let Some(prev) = last_numbered {
            let gap = hunk.new_start.saturating_sub(prev.saturating_add(1));
            if gap > 0 {
                let _ = writeln!(text, "... {gap} lines omitted ...");
            }
        }

        for (number, line) in hunk.numbered_lines() {
            match number {
                Some(n) => {
                    let _ = writeln!(text, "{}{n:>6} | {}", line.marker(), line.content());
                    valid_lines.insert(n);
                    last_numbered = Some(n);
                }
                // A removed line: six spaces where the number goes. It has no
                // line in the new file, and inventing one is the error this
                // whole format exists to prevent.
                None => {
                    let _ = writeln!(text, "{}{:>6} | {}", line.marker(), "", line.content());
                }
            }
        }
    }

    Some(Payload { text, valid_lines })
}

/// Pick the right scope sentence for this set of hunks.
///
/// Whole-file mode (every line is `Context`) tells the model to review the
/// whole file; diff mode (some line is `Added` or `Removed`) tells it to focus
/// on the marked lines and never report findings on removed lines. The
/// decision reads from the data so the caller cannot get a flag wrong; it is
/// sound because git never emits a hunk with no changed line, as noted on
/// [`Hunk::whole_file`].
fn scope_sentence(hunks: &[&Hunk]) -> &'static str {
    let has_change = hunks
        .iter()
        .any(|h| h.lines.iter().any(|l| l.marker() != ' '));
    if has_change {
        "Review the lines marked `+`. Lines with no marker are unchanged context. \
         Lines marked `-` were removed and have no line number; do not report \
         findings on them. Report each finding using the line number shown in the gutter."
    } else {
        "Review the entire file. Report each finding using the line number shown in the gutter."
    }
}
