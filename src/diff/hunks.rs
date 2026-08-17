//! Diff hunks: the structured form of a unified diff, and the parser that
//! produces it.
//!
//! A single `git diff --unified=N` output is just text, but everything that
//! consumes it wants the same shape: a list of hunks, each tagged with its
//! file and the line ranges it touches, and each line tagged with whether it
//! was added, removed, or context. Parsing this once, here, means the
//! callers (`mod.rs` queries, the payload renderer) never re-parse git's
//! output and never have to ask "what does `+++ /dev/null` mean again".
//!
//! The parser is deliberately tolerant. A diff that cannot be parsed must
//! not take the gate down — it is much better to ship a partial answer
//! than no answer — so anything not recognised is skipped rather than
//! erroring. The load-bearing rules (where the file path comes from, how
//! each line is tagged) are documented inline.

use std::path::PathBuf;

use crate::files;

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
    /// Every line that exists in the new file, paired with its new-file line
    /// number. Context and Added lines only — Removed lines are skipped
    /// because they do not exist in the new file and so have no number.
    ///
    /// Numbering starts at `new_start` and increments for each Context or
    /// Added line, in order. Removed lines do not advance it; that is the
    /// invariant the payload depends on for `valid_lines`, and the reason
    /// the renderer can attribute findings to the right code.
    pub fn numbered_new_lines(&self) -> Vec<(u32, &str)> {
        let mut out = Vec::with_capacity(self.lines.len());
        let mut current = self.new_start;
        for line in &self.lines {
            match line {
                HunkLine::Context(s) | HunkLine::Added(s) => {
                    out.push((current, s.as_str()));
                    current = current.saturating_add(1);
                }
                HunkLine::Removed(_) => {}
            }
        }
        out
    }

    /// Build a synthetic hunk covering an entire file's content, for
    /// `drep check PATHS` where there is no diff to consult. Every line is
    /// `Context`, numbering starts at 1, so the payload renderer can use
    /// the same `numbered_new_lines` walk and the same gap logic — it
    /// switches to the whole-file scope sentence because no line is added
    /// or removed.
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
/// The list of intentionally accepted quirks follows the rules in the spec:
/// the file path comes from `+++ b/...`, never from `diff --git a/... b/...`
/// (a `b/` substring in the repo path would make any rule that finds it on
/// the git header capture the wrong span); a `+++ /dev/null` line means a
/// deletion and the whole file is skipped; `\ No newline at end of file` is
/// not a `HunkLine`; and a malformed `@@` line terminates the current hunk
/// without attributing its body to the previous one.
pub fn parse_unified_diff(diff_text: &str) -> Vec<Hunk> {
    if diff_text.trim().is_empty() {
        return Vec::new();
    }

    let mut hunks: Vec<Hunk> = Vec::new();
    let mut current_file: Option<PathBuf> = None;
    let mut skipping_file = false;
    let mut pending: Option<PendingHunk> = None;
    let mut skip_until_next_hunk = false;

    for line in diff_text.lines() {
        if line.starts_with("diff --git ") {
            finish_hunk(&mut pending, &mut hunks);
            current_file = None;
            skipping_file = false;
            skip_until_next_hunk = false;
            continue;
        }

        if skipping_file {
            continue;
        }

        if let Some(after_marker) = line.strip_prefix("+++ b/") {
            let path = PathBuf::from(after_marker);
            if files::is_scan_target(&path) {
                current_file = Some(path);
                skip_until_next_hunk = false;
            } else {
                finish_hunk(&mut pending, &mut hunks);
                current_file = None;
                skipping_file = true;
            }
            continue;
        }

        if line.starts_with("+++ /dev/null") {
            finish_hunk(&mut pending, &mut hunks);
            current_file = None;
            skipping_file = true;
            continue;
        }

        if let Some(after_marker) = line.strip_prefix("@@") {
            // Hunk-body terminator per rule 7, regardless of whether the
            // header shape is valid — that is the whole point of the
            // malformed-`@@` rule (rule 11): a non-header `@@` must not
            // have its body attributed to the previous hunk.
            finish_hunk(&mut pending, &mut hunks);

            if let Some((old_start, old_count, new_start, new_count)) =
                parse_hunk_header(after_marker)
            {
                if let Some(file_path) = current_file.clone() {
                    pending = Some(PendingHunk::new(
                        file_path, old_start, old_count, new_start, new_count,
                    ));
                    skip_until_next_hunk = false;
                }
                // No `+++` line for this file: nothing to attribute the
                // hunk to, so it is dropped silently rather than left
                // dangling. Same outcome as a non-target file.
            } else {
                skip_until_next_hunk = true;
            }
            continue;
        }

        if skip_until_next_hunk {
            continue;
        }

        let Some(hunk) = pending.as_mut() else {
            continue;
        };

        // Inside a hunk body the first character alone decides the kind;
        // this is the one place the parser looks at content beyond the
        // initial marker. Everything else (line continuations, embedded
        // `---` / `+++` characters) is preserved verbatim in the source.
        match line.as_bytes().first() {
            Some(b'\\') => {
                // `\ No newline at end of file` — refers to the previous
                // line, not a content line itself.
            }
            Some(b'+') => hunk.push_added(&line[1..]),
            Some(b'-') => hunk.push_removed(&line[1..]),
            Some(b' ') => hunk.push_context(&line[1..]),
            Some(_) => {
                // A line that does not match any marker is impossible in
                // a well-formed hunk body. Treat it the same way git
                // would treat a `\` line and continue — never error.
            }
            None => {
                // Empty line: same handling as a non-matching line.
            }
        }
    }

    finish_hunk(&mut pending, &mut hunks);
    hunks
}

/// A hunk being accumulated before it is emitted.
///
/// The lifetimes are tied to the parser loop, so there is no need for arena
/// allocation here — the parser owns the single in-flight hunk at a time.
struct PendingHunk {
    file_path: PathBuf,
    old_start: u32,
    old_count: u32,
    new_start: u32,
    new_count: u32,
    lines: Vec<HunkLine>,
}

impl PendingHunk {
    fn new(
        file_path: PathBuf,
        old_start: u32,
        old_count: u32,
        new_start: u32,
        new_count: u32,
    ) -> Self {
        Self {
            file_path,
            old_start,
            old_count,
            new_start,
            new_count,
            lines: Vec::new(),
        }
    }

    fn push_context(&mut self, content: &str) {
        self.lines.push(HunkLine::Context(content.to_owned()));
    }

    fn push_added(&mut self, content: &str) {
        self.lines.push(HunkLine::Added(content.to_owned()));
    }

    fn push_removed(&mut self, content: &str) {
        self.lines.push(HunkLine::Removed(content.to_owned()));
    }

    fn finalize(self) -> Hunk {
        Hunk {
            file_path: self.file_path,
            old_start: self.old_start,
            old_count: self.old_count,
            new_start: self.new_start,
            new_count: self.new_count,
            lines: self.lines,
        }
    }
}

fn finish_hunk(pending: &mut Option<PendingHunk>, hunks: &mut Vec<Hunk>) {
    if let Some(hunk) = pending.take() {
        hunks.push(hunk.finalize());
    }
}

/// Parse the middle of a `@@` line: `-old[,oc] +new[,nc]`, possibly followed
/// by anything up to the closing `@@` (git's function-name guess).
///
/// Returns `None` for anything that does not match the header shape, which
/// the parser treats as a malformed `@@` — the body that follows is
/// skipped, not appended to the previous hunk. The closing `@@` is required:
/// a line that looks like `-1,3 +1,4` with no trailing `@@` is not a hunk
/// header, and accepting it would let junk lines pass as start-of-hunk.
fn parse_hunk_header(after_marker: &str) -> Option<(u32, u32, u32, u32)> {
    let after_marker = after_marker.trim_start();
    let old_part = after_marker.strip_prefix('-')?;
    let old = parse_range_with_tail(old_part)?;

    let tail = old.tail.trim_start();
    let new_part = tail.strip_prefix('+')?;
    let new = parse_range_with_tail(new_part)?;

    let after_new = new.tail.trim_start();
    after_new.strip_prefix("@@")?;

    Some((old.start, old.count, new.start, new.count))
}

struct RangeTail {
    start: u32,
    count: u32,
    tail: String,
}

/// Parse `<number>[,<count>]` and return the unconsumed suffix.
///
/// An omitted count is treated as 1 — matches git's convention for
/// `@@ -5 +5 @@` (a single-line hunk). `count == 0` is legal and is what
/// appears for new and emptied files; it is preserved verbatim.
fn parse_range_with_tail(s: &str) -> Option<RangeTail> {
    let (start_str, rest) = match s.find(|c: char| c == ',' || c.is_whitespace()) {
        Some(idx) => (&s[..idx], &s[idx..]),
        None => (s, ""),
    };
    let start: u32 = start_str.parse().ok()?;

    let (count, tail) = if let Some(after_comma) = rest.strip_prefix(',') {
        let end = after_comma
            .find(|c: char| c == ',' || c.is_whitespace())
            .unwrap_or(after_comma.len());
        let count_str = &after_comma[..end];
        let count: u32 = count_str.parse().ok()?;
        (count, after_comma[end..].to_owned())
    } else {
        (1, rest.to_owned())
    };

    Some(RangeTail { start, count, tail })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_returns_no_hunks() {
        assert!(parse_unified_diff("").is_empty());
        assert!(parse_unified_diff("   \n\n  \n").is_empty());
    }

    #[test]
    fn hunk_header_with_full_counts() {
        let parsed = parse_hunk_header(" -1,3 +1,4 @@").expect("header");
        assert_eq!(parsed, (1, 3, 1, 4));
    }

    #[test]
    fn hunk_header_with_omitted_counts_defaults_to_one() {
        let parsed = parse_hunk_header(" -5 +5 @@").expect("header");
        assert_eq!(parsed, (5, 1, 5, 1));
    }

    #[test]
    fn hunk_header_with_zero_counts_is_legal() {
        let parsed = parse_hunk_header(" -0,0 +1,3 @@").expect("header");
        assert_eq!(parsed, (0, 0, 1, 3));
    }

    #[test]
    fn hunk_header_allows_trailing_function_name() {
        let parsed = parse_hunk_header(" -1,3 +1,4 @@ fn compute()").expect("header");
        assert_eq!(parsed, (1, 3, 1, 4));
    }

    #[test]
    fn malformed_hunk_header_returns_none() {
        assert!(parse_hunk_header("garbage").is_none());
        assert!(parse_hunk_header(" -1,3 +1,4").is_none());
        assert!(parse_hunk_header(" -a +1 @@").is_none());
    }
}
