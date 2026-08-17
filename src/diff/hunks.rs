//! Diff hunks: the structured form of a unified diff, and the parser that
//! produces it.
//!
//! A single `git diff --unified=N` output is just text, but everything that
//! consumes it wants the same shape: a list of hunks, each tagged with its
//! file and the line ranges it touches, and each line tagged with whether it
//! was added, removed, or context. Parsing this once, here, means the callers
//! (`mod.rs` queries, the payload renderer) never re-parse git's output and
//! never have to ask "what does `+++ /dev/null` mean again".
//!
//! The parser is deliberately tolerant. A diff that cannot be parsed must not
//! take the gate down — it is much better to ship a partial answer than no
//! answer — so anything not recognised is skipped rather than erroring.
//!
//! Deliberately **free of drep policy**: this module answers "what does this
//! diff say", not "which files does drep review". Whether a path is worth
//! analyzing is a product decision, and it lives with the other git-semantics
//! decisions in `mod.rs` beside `filter_scan_targets`. Keeping it out of here
//! is what lets the parser serve a future caller with a different scope (an
//! `include`/`exclude` config, `lint-docs`) without threading a predicate
//! through it.

use std::path::PathBuf;

/// One line inside a hunk, tagged by what the diff said about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HunkLine {
    /// Unchanged line, present in both old and new file.
    Context(String),
    /// Line present only in the new file.
    Added(String),
    /// Line present only in the old file. Has no new-file line number.
    Removed(String),
}

impl HunkLine {
    /// The gutter marker for this line kind, matching unified-diff notation.
    ///
    /// Lives on the type rather than in the renderer so that "which character
    /// means removed" is answered once. The renderer pairs it with
    /// [`Hunk::numbered_lines`], and the two together are the whole gutter.
    pub const fn marker(&self) -> char {
        match self {
            HunkLine::Context(_) => ' ',
            HunkLine::Added(_) => '+',
            HunkLine::Removed(_) => '-',
        }
    }

    /// The line's text, with the diff's leading marker already stripped.
    pub fn content(&self) -> &str {
        match self {
            HunkLine::Context(s) | HunkLine::Added(s) | HunkLine::Removed(s) => s,
        }
    }
}

/// One `@@` hunk from a unified diff, with its file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    pub file_path: PathBuf,
    pub old_start: u32,
    pub old_count: u32,
    pub new_start: u32,
    pub new_count: u32,
    pub lines: Vec<HunkLine>,
}

impl Hunk {
    /// Every line in the hunk paired with its new-file line number, or `None`
    /// for a `Removed` line.
    ///
    /// **This is the single implementation of the line-numbering rule**, and
    /// everything that needs a line number goes through it. Numbering starts
    /// at `new_start` and advances for each `Context` or `Added` line;
    /// `Removed` lines do not advance it, because they do not exist in the new
    /// file at all.
    ///
    /// That rule is what `Payload::valid_lines` rests on, and therefore what
    /// decides whether an LLM finding gets attributed to the right code. It
    /// previously existed twice — once here and once inline in the renderer —
    /// which meant hardening one did nothing for the other.
    pub fn numbered_lines(&self) -> impl Iterator<Item = (Option<u32>, &HunkLine)> {
        let mut next = self.new_start;
        self.lines.iter().map(move |line| match line {
            HunkLine::Removed(_) => (None, line),
            HunkLine::Context(_) | HunkLine::Added(_) => {
                let number = next;
                next = next.saturating_add(1);
                (Some(number), line)
            }
        })
    }

    /// Just the lines that exist in the new file, with their line numbers.
    ///
    /// A projection of [`Self::numbered_lines`], not a second walk: callers
    /// that only care about real file lines (checking a parse against the file
    /// on disk, say) get them without restating how numbering works.
    pub fn numbered_new_lines(&self) -> impl Iterator<Item = (u32, &str)> {
        self.numbered_lines()
            .filter_map(|(number, line)| number.map(|n| (n, line.content())))
    }

    /// Build a synthetic hunk covering an entire file's content, for
    /// `drep check PATHS` where there is no diff to consult.
    ///
    /// Every line is `Context` and numbering starts at 1, so the renderer
    /// walks it with the same `numbered_lines` iterator it uses for a real
    /// hunk. It also selects the whole-file scope sentence, because no line is
    /// added or removed — an inference that holds because **git never emits a
    /// hunk with no changed line**: hunks exist only around changes. That
    /// property is load-bearing and is stated here because it belongs to an
    /// external tool rather than to this type.
    pub fn whole_file(file_path: PathBuf, content: &str) -> Hunk {
        let lines: Vec<HunkLine> = content
            .lines()
            .map(|line| HunkLine::Context(line.to_owned()))
            .collect();
        let new_count = lines.len() as u32;
        Hunk {
            file_path,
            old_start: 0,
            old_count: 0,
            new_start: 1,
            new_count,
            lines,
        }
    }
}

/// Parse the output of `git diff --unified=N` into hunks.
///
/// Tolerant by design: anything it does not recognise is skipped rather than
/// erroring, because a diff that cannot be parsed must not take the gate down.
/// The intentional quirks:
///
/// - **The file path comes from `+++ b/…`, never from `diff --git a/… b/…`.**
///   The git header carries two paths on one line with no unambiguous
///   separator, so any "find `b/`" rule captures the wrong span for a
///   repository path that itself contains `b/` (`src/b/mod.rs`). The Python
///   `diff_parser.py` this replaces had exactly that bug.
/// - `+++ /dev/null` marks a deletion; there is nothing to analyze, so the
///   file's hunks are dropped.
/// - **Inside a hunk body the first byte alone decides the line kind.** Lines
///   starting `---` or `+++` are *not* additionally skipped: those headers
///   appear only before the first `@@` of a file, and a removed source line
///   whose own text begins with `--` arrives as `---…`. Skipping it silently
///   drops real removed code — the second bug in the Python.
/// - `\ No newline at end of file` refers to the preceding line and never
///   becomes a `HunkLine`.
/// - A malformed `@@` terminates the current hunk without its body being
///   attributed to the previous one.
pub fn parse_unified_diff(diff_text: &str) -> Vec<Hunk> {
    if diff_text.trim().is_empty() {
        return Vec::new();
    }

    let mut hunks: Vec<Hunk> = Vec::new();
    // `None` means "no file to attribute a hunk to" — either we have not
    // reached this file's `+++` line yet, or it was a deletion. A `@@` seen
    // while it is `None` produces no hunk, which is what drops a deleted
    // file's body without a separate flag to track.
    let mut current_file: Option<PathBuf> = None;
    let mut pending: Option<Hunk> = None;

    for line in diff_text.lines() {
        if line.starts_with("diff --git ") {
            hunks.extend(pending.take());
            current_file = None;
            continue;
        }

        if let Some(path) = line.strip_prefix("+++ b/") {
            current_file = Some(PathBuf::from(path));
            continue;
        }

        if line.starts_with("+++ /dev/null") {
            hunks.extend(pending.take());
            current_file = None;
            continue;
        }

        if let Some(after_marker) = line.strip_prefix("@@") {
            // Terminates the body regardless of whether the header parses:
            // that is what stops a malformed `@@`'s lines being appended to
            // the hunk before it.
            hunks.extend(pending.take());
            pending = parse_hunk_header(after_marker).and_then(|(os, oc, ns, nc)| {
                current_file.clone().map(|file_path| Hunk {
                    file_path,
                    old_start: os,
                    old_count: oc,
                    new_start: ns,
                    new_count: nc,
                    lines: Vec::new(),
                })
            });
            continue;
        }

        let Some(hunk) = pending.as_mut() else {
            continue;
        };

        match line.as_bytes().first() {
            Some(b'+') => hunk.lines.push(HunkLine::Added(line[1..].to_owned())),
            Some(b'-') => hunk.lines.push(HunkLine::Removed(line[1..].to_owned())),
            Some(b' ') => hunk.lines.push(HunkLine::Context(line[1..].to_owned())),
            // `\ No newline at end of file`, a blank line, or anything else a
            // well-formed body cannot contain. Never an error.
            _ => {}
        }
    }

    hunks.extend(pending.take());
    hunks
}

/// Parse the middle of a `@@` line: `-old[,oc] +new[,nc]`, followed by the
/// closing `@@` and optionally git's guessed function signature.
///
/// The closing `@@` is required: `-1,3 +1,4` with nothing after it is not a
/// hunk header, and accepting it would let junk pass as a start-of-hunk.
/// `split_once` takes the *first* `@@`, so a function signature that itself
/// contains `@@` cannot extend the range span.
fn parse_hunk_header(after_marker: &str) -> Option<(u32, u32, u32, u32)> {
    let (ranges, _signature) = after_marker.trim_start().split_once("@@")?;
    let mut parts = ranges.split_whitespace();
    let (old_start, old_count) = parse_range(parts.next()?.strip_prefix('-')?)?;
    let (new_start, new_count) = parse_range(parts.next()?.strip_prefix('+')?)?;
    // A third range is not a hunk header; refusing it keeps the malformed-`@@`
    // path reachable rather than silently accepting junk.
    if parts.next().is_some() {
        return None;
    }
    Some((old_start, old_count, new_start, new_count))
}

/// Parse `<start>[,<count>]`.
///
/// An omitted count is 1, matching git's convention for a single-line hunk
/// (`@@ -5 +5 @@`). A count of 0 is legal and is what appears for new and
/// emptied files.
fn parse_range(s: &str) -> Option<(u32, u32)> {
    match s.split_once(',') {
        Some((start, count)) => Some((start.parse().ok()?, count.parse().ok()?)),
        None => Some((s.parse().ok()?, 1)),
    }
}

#[cfg(test)]
mod tests {
    //! Tests for items private to this module, which the sibling
    //! `diff::tests::hunks` cannot reach. Everything exercised through the
    //! public API lives there instead.

    use super::*;

    #[test]
    fn hunk_header_with_full_counts() {
        assert_eq!(parse_hunk_header(" -1,3 +1,4 @@"), Some((1, 3, 1, 4)));
    }

    #[test]
    fn hunk_header_with_omitted_counts_defaults_to_one() {
        assert_eq!(parse_hunk_header(" -5 +5 @@"), Some((5, 1, 5, 1)));
    }

    #[test]
    fn hunk_header_with_zero_counts_is_legal() {
        assert_eq!(parse_hunk_header(" -0,0 +1,3 @@"), Some((0, 0, 1, 3)));
    }

    #[test]
    fn hunk_header_allows_a_trailing_function_signature() {
        assert_eq!(
            parse_hunk_header(" -1,3 +1,4 @@ fn compute()"),
            Some((1, 3, 1, 4))
        );
    }

    #[test]
    fn hunk_header_signature_containing_at_at_does_not_extend_the_ranges() {
        // `split_once` must take the first `@@`, not the last.
        assert_eq!(
            parse_hunk_header(" -1,3 +1,4 @@ fn f() { \"@@\" }"),
            Some((1, 3, 1, 4))
        );
    }

    #[test]
    fn malformed_hunk_headers_are_rejected() {
        assert!(parse_hunk_header("garbage").is_none());
        // No closing `@@`.
        assert!(parse_hunk_header(" -1,3 +1,4").is_none());
        // Non-numeric start.
        assert!(parse_hunk_header(" -a +1 @@").is_none());
        // A third range.
        assert!(parse_hunk_header(" -1,3 +1,4 +9,9 @@").is_none());
        // A count that is not a number.
        assert!(parse_hunk_header(" -1,3,5 +1,4 @@").is_none());
    }

    #[test]
    fn range_without_a_comma_has_an_implicit_count_of_one() {
        assert_eq!(parse_range("42"), Some((42, 1)));
        assert_eq!(parse_range("42,7"), Some((42, 7)));
        assert!(parse_range("").is_none());
    }
}
