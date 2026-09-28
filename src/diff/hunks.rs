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
//! decisions in `mod.rs` beside `filter_paths`. Keeping it out of here
//! is what lets the parser serve a future caller with a different scope (an
//! `include`/`exclude` config, `lint-docs`) without threading a predicate
//! through it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::quoting::decode;

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

/// Group owned hunks by file in deterministic path order.
///
/// Shared by input resolution and the analyzer's defensive public boundary so
/// the normal grouping rule cannot diverge from its release-mode recovery.
pub(crate) fn group_by_file(hunks: impl IntoIterator<Item = Hunk>) -> Vec<Vec<Hunk>> {
    let mut by_file: BTreeMap<PathBuf, Vec<Hunk>> = BTreeMap::new();
    for hunk in hunks {
        by_file
            .entry(hunk.file_path.clone())
            .or_default()
            .push(hunk);
    }
    by_file.into_values().collect()
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
/// The rules, which follow git's own reader (`apply.c`):
///
/// - **A hunk body is exactly as long as its `@@` counts say.** Context
///   consumes a line from each side, a removal one old line, an addition one
///   new line, and `\ No newline at end of file` none; that marker refers to
///   the preceding line and never becomes a `HunkLine`. Every body line is
///   prefixed, so no content can end a body early: an added source line that
///   reads `++ /dev/null` arrives as `+++ /dev/null` and is still an addition,
///   not a deletion header that hides the rest of the file from review, and a
///   removed one reading `-- x` arrives as `--- x` and is still a removal.
/// - An empty body line is an empty context line, as `diff.suppressBlankEmpty`
///   prints it. Any other unprefixed line, or one the counts leave no room
///   for, ends the hunk early and is then read as a header.
/// - **The file path comes from the `+++` label, never from `diff --git`.**
///   That line carries two paths with no unambiguous separator, so any "find
///   `b/`" rule captures the wrong span for a repository path that itself
///   contains `b/` (`src/b/mod.rs`). The label is decoded as git quoted it, and
///   the tab git appends to a name containing a space is dropped - see
///   `new_file_path`. `+++ /dev/null` marks a deletion, which has nothing to
///   analyze, so its hunks are dropped.
/// - A malformed `@@` produces no hunk, and nothing after it is attributed to
///   the hunk before it.
pub fn parse_unified_diff(diff_text: &str) -> Vec<Hunk> {
    let mut hunks: Vec<Hunk> = Vec::new();
    // `None` means "no file to attribute a hunk to" - either we have not
    // reached this file's `+++` line yet, or it was a deletion. A `@@` seen
    // while it is `None` produces no hunk, which is what drops a deleted
    // file's body without a separate flag to track.
    let mut current_file: Option<PathBuf> = None;
    let mut body: Option<Body> = None;

    for line in diff_text.lines() {
        if let Some(open) = body.as_mut() {
            if open.accept(line) {
                if open.is_complete() {
                    hunks.extend(body.take().map(Body::finish));
                }
                continue;
            }
            hunks.extend(body.take().map(Body::finish));
        }

        if line.starts_with("diff --git ") {
            current_file = None;
        } else if let Some(label) = line.strip_prefix("+++ ") {
            current_file = new_file_path(label);
        } else if let Some(after_marker) = line.strip_prefix("@@") {
            body = parse_hunk_header(after_marker).and_then(|(os, oc, ns, nc)| {
                current_file.clone().map(|file_path| Body {
                    hunk: Hunk {
                        file_path,
                        old_start: os,
                        old_count: oc,
                        new_start: ns,
                        new_count: nc,
                        lines: Vec::new(),
                    },
                    old_left: oc,
                    new_left: nc,
                })
            });
        }
    }

    hunks.extend(body.map(Body::finish));
    hunks
}

/// A hunk whose body is still being read, with the lines each side of its
/// `@@` header has left to account for.
struct Body {
    hunk: Hunk,
    old_left: u32,
    new_left: u32,
}

impl Body {
    /// Take `line` into the body, or `false` when it cannot belong there.
    fn accept(&mut self, line: &str) -> bool {
        let (entry, old, new) = match line.as_bytes().first() {
            Some(b' ') => (HunkLine::Context(line[1..].to_owned()), 1, 1),
            None => (HunkLine::Context(String::new()), 1, 1),
            Some(b'-') => (HunkLine::Removed(line[1..].to_owned()), 1, 0),
            Some(b'+') => (HunkLine::Added(line[1..].to_owned()), 0, 1),
            Some(b'\\') => return true,
            Some(_) => return false,
        };
        let (Some(old_left), Some(new_left)) = (
            self.old_left.checked_sub(old),
            self.new_left.checked_sub(new),
        ) else {
            return false;
        };
        self.old_left = old_left;
        self.new_left = new_left;
        self.hunk.lines.push(entry);
        true
    }

    fn is_complete(&self) -> bool {
        self.old_left == 0 && self.new_left == 0
    }

    fn finish(self) -> Hunk {
        self.hunk
    }
}

/// The file a `+++ ` header names, or `None` when it names no new file.
///
/// Git ends the header with a tab whenever the name holds a space, and an
/// unquoted name never holds a tab (git quotes one), so a single trailing tab is
/// always that marker. The `b/` prefix is removed after decoding because a
/// quoted field carries it inside the quotes. `/dev/null`, a deletion, has no
/// such prefix, and neither does a header written without one: the parser does
/// not guess where a prefix it cannot see would have ended.
fn new_file_path(target: &str) -> Option<PathBuf> {
    let field = target.strip_suffix('\t').unwrap_or(target);
    decode(field).strip_prefix("b").ok().map(Path::to_path_buf)
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
