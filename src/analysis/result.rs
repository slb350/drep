//! What one analysis pass produced.
//!
//! A single pass over a file can produce findings AND fail to fully analyze
//! the file (a truncated response gives a partial list, an unknown severity
//! in one record fails the file, a transport error produces zero findings but
//! still surfaces as a failure). Reporting the findings while forgetting the
//! failure is the exact bug this type exists to prevent — the gate would
//! green-light a commit whenever the LLM endpoint was unreachable, which is
//! worse than having no gate at all.
//!
//! `failed_files` is a [`BTreeMap`] rather than a `Vec` because two passes
//! over the same file set must UNION, never sum. Summing counts one
//! unreachable endpoint twice, drifting the failure count up without any
//! matching file to investigate. The map's value carries the reason so the
//! caller can render something a user can act on, not just a path.
//!
//! `dropped_out_of_range` counts rather than silently drops out-of-range
//! findings so that a model which consistently reports wrong lines is
//! observable to the caller — not invisible.

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

use crate::analysis::findings::Finding;

/// Why one file went unanalyzed.
///
/// A bare set of paths cannot tell a dead endpoint from a rate limit from a
/// truncated response, and the caller needs that to print something a user can
/// act on. `LlmError` already carries the detail; it used to be discarded at
/// the analyzer boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FailureReason {
    /// The endpoint was unreachable, or returned a retryable status too many
    /// times. `status` is the HTTP code when there was one.
    Transport {
        status: Option<u16>,
        message: String,
    },
    /// A response arrived and no JSON could be extracted from it.
    Unparseable(String),
    /// The response parsed only after closing unbalanced delimiters, so it is
    /// a prefix of what the model meant to say.
    Truncated,
    /// A record in the response could not be understood - unknown severity,
    /// missing field, unusable line number.
    MalformedFinding(String),
    /// A deterministic tool that should have run could not.
    ToolUnavailable { tool: String, detail: String },
    /// The file on disk exceeded the read guard, so drep never read it.
    ///
    /// Distinct from [`Self::PayloadTooLarge`] because the two measure
    /// different things: this is the file's own size, checked before any I/O,
    /// and that one is the size of the text the model would have been sent.
    /// They were one variant sharing one limit, which meant `bytes` held the
    /// file size on one code path and the rendered-payload size on another -
    /// so "file is too large (330102 bytes)" could name a file that `ls`
    /// reports as 261900 bytes.
    FileTooLarge { bytes: u64, limit: u64 },
    /// The rendered LLM payload exceeded the ceiling. See [`Self::FileTooLarge`].
    PayloadTooLarge { bytes: u64, limit: u64 },
    /// The file could not be read from disk.
    Unreadable(String),
}

impl FailureReason {
    /// A single line suitable for a terminal, derived from the variant.
    ///
    /// The HTTP status is rendered next to the message so a 429 is visible
    /// without the user having to match the message against a status code
    /// list. This is the load-bearing reason the `Transport` variant carries
    /// the status as a number rather than only inside the string.
    pub fn one_line(&self) -> String {
        match self {
            FailureReason::Transport {
                status: Some(code),
                message,
            } => {
                format!("LLM transport failed (HTTP {code}): {message}")
            }
            FailureReason::Transport {
                status: None,
                message,
            } => {
                format!("LLM transport failed: {message}")
            }
            FailureReason::Unparseable(message) => {
                format!("LLM response was unparseable: {message}")
            }
            FailureReason::Truncated => "response was truncated".to_owned(),
            FailureReason::MalformedFinding(detail) => format!("malformed finding: {detail}"),
            FailureReason::ToolUnavailable { tool, detail } => {
                format!("{tool} could not run: {detail}")
            }
            FailureReason::FileTooLarge { bytes, limit } => {
                format!("file is too large to read ({bytes} bytes; limit is {limit})")
            }
            FailureReason::PayloadTooLarge { bytes, limit } => {
                format!("the code sent for review is too large ({bytes} bytes; limit is {limit})")
            }
            FailureReason::Unreadable(detail) => format!("file could not be read: {detail}"),
        }
    }

    /// The HTTP status, when the failure had one.
    ///
    /// Only `Transport` ever carries one. Exposed as a number rather than
    /// left inside the message because a caller has to distinguish a 429 from
    /// a 401 - the message is prose and prose gets reworded.
    pub fn status(&self) -> Option<u16> {
        match self {
            FailureReason::Transport { status, .. } => *status,
            _ => None,
        }
    }
}

impl fmt::Display for FailureReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.one_line())
    }
}

/// What one analysis pass produced.
///
/// `findings` and `failed_files` are independent axes: a file can contribute
/// findings AND be unanalyzed (a truncated response gives a partial list).
/// Reporting the findings while forgetting the failure is the exact bug this
/// type exists to prevent.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct AnalysisResult {
    /// Findings the analyzer could attribute to a real line of code.
    pub findings: Vec<Finding>,
    /// Files that could not be fully analyzed, with the reason. A `BTreeMap`
    /// because two passes over the same file set must UNION, never sum —
    /// summing counts one unreachable endpoint twice.
    pub failed_files: BTreeMap<PathBuf, FailureReason>,
    /// Findings discarded because their line was not in the payload's
    /// `valid_lines`. Counted rather than silently dropped, so the drop is
    /// observable.
    pub dropped_out_of_range: usize,
}

/// Fold `src` into `dst`, keeping the reason already present on a collision.
///
/// The one statement of the failure-union rule. It was written out longhand as
/// `entry().or_insert()` at four sites - `merge` here plus three in the CLI -
/// each with its own comment re-explaining it. The two analysis layers cover
/// the same files, so the sets union rather than sum: one unreachable endpoint
/// is one failure, not two. First-wins because the reasons cannot be
/// meaningfully combined and the earlier layer saw the file first.
pub fn union_failures(
    dst: &mut BTreeMap<PathBuf, FailureReason>,
    src: BTreeMap<PathBuf, FailureReason>,
) {
    for (path, reason) in src {
        dst.entry(path).or_insert(reason);
    }
}

impl AnalysisResult {
    /// One file, one failure, no findings.
    ///
    /// The shape was hand-assembled at four call sites - `default()`, insert,
    /// return - each of which independently had to know that `findings` and
    /// `dropped_out_of_range` stay at their defaults. Forgetting the insert at
    /// any one of them reports an unanalyzed file as clean, which is the single
    /// failure this whole type exists to prevent, so it gets a constructor.
    pub fn failed(path: PathBuf, reason: FailureReason) -> Self {
        let mut result = Self::default();
        result.failed_files.insert(path, reason);
        result
    }

    /// Fold `other` into `self`: findings concatenate, `failed_files`
    /// unions, `dropped_out_of_range` sums.
    ///
    /// The merge semantics are what let a caller run several analyzers
    /// (code quality, docstrings, ...) and combine their results without
    /// losing the failure signal. Calling `merge` in a loop over per-file
    /// results inside `analyze_files` is the planned usage.
    ///
    /// On a key collision in `failed_files`, the **first** reason wins. A
    /// file failing twice is still one failure, and the two reasons are not
    /// meaningfully combinable - the first one is at least specific to the
    /// file, while a hypothetical last-wins policy would let a later
    /// analyzer overwrite a more informative first reason with a generic
    /// one.
    pub fn merge(&mut self, other: AnalysisResult) {
        self.findings.extend(other.findings);
        for (path, reason) in other.failed_files {
            self.failed_files.entry(path).or_insert(reason);
        }
        self.dropped_out_of_range = self
            .dropped_out_of_range
            .saturating_add(other.dropped_out_of_range);
    }

    /// True when any file went unanalyzed.
    ///
    /// The caller maps this to process exit 2: "could not analyze" is
    /// distinct from both "clean" (exit 0) and "found issues" (exit 1),
    /// because a gate that cannot distinguish them rubber-stamps the day
    /// the LLM endpoint goes down.
    pub fn has_failures(&self) -> bool {
        !self.failed_files.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `merge` keeps the first reason on a key collision - the documented
    /// first-writer-wins rule. A last-wins policy would silently overwrite
    /// the more informative first reason with a generic later one.
    #[test]
    fn merge_keeps_first_reason_on_key_collision() {
        let mut a = AnalysisResult::default();
        a.failed_files.insert(
            PathBuf::from("src/lib.rs"),
            FailureReason::Transport {
                status: Some(429),
                message: "rate limited".to_owned(),
            },
        );

        let mut b = AnalysisResult::default();
        b.failed_files.insert(
            PathBuf::from("src/lib.rs"),
            FailureReason::Transport {
                status: Some(500),
                message: "internal".to_owned(),
            },
        );

        a.merge(b);

        assert_eq!(a.failed_files.len(), 1);
        let reason = a.failed_files.get(&PathBuf::from("src/lib.rs")).unwrap();
        assert_eq!(
            reason,
            &FailureReason::Transport {
                status: Some(429),
                message: "rate limited".to_owned(),
            },
            "first reason wins on collision"
        );
    }

    /// A `Transport` failure with a status surfaces the code in the rendered
    /// line. The whole point of keeping the status as a number is that it
    /// reaches the user; this pins that the rendering preserves it.
    #[test]
    fn transport_render_includes_the_http_status() {
        let reason = FailureReason::Transport {
            status: Some(429),
            message: "rate limited".to_owned(),
        };
        let rendered = reason.one_line();
        assert!(
            rendered.contains("429"),
            "rendered line must contain 429, got {rendered:?}"
        );
    }
}
