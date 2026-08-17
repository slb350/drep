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
//! `failed_files` is a [`BTreeSet`] rather than a `Vec` because two passes
//! over the same file set must UNION, never sum. Summing counts one
//! unreachable endpoint twice, drifting the failure count up without any
//! matching file to investigate.
//!
//! `dropped_out_of_range` counts rather than silently drops out-of-range
//! findings so that a model which consistently reports wrong lines is
//! observable to the caller — not invisible.

use std::collections::BTreeSet;
use std::path::PathBuf;

use crate::analysis::findings::Finding;

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
    /// Files that could not be fully analyzed. A `BTreeSet` because two
    /// passes over the same file set must UNION, never sum — summing counts
    /// one unreachable endpoint twice.
    pub failed_files: BTreeSet<PathBuf>,
    /// Findings discarded because their line was not in the payload's
    /// `valid_lines`. Counted rather than silently dropped, so the drop is
    /// observable.
    pub dropped_out_of_range: usize,
}

impl AnalysisResult {
    /// Fold `other` into `self`: findings concatenate, `failed_files`
    /// unions, `dropped_out_of_range` sums.
    ///
    /// The merge semantics are what let a caller run several analyzers
    /// (code quality, docstrings, ...) and combine their results without
    /// losing the failure signal. Calling `merge` in a loop over per-file
    /// results inside `analyze_files` is the planned usage.
    pub fn merge(&mut self, other: AnalysisResult) {
        self.findings.extend(other.findings);
        self.failed_files.extend(other.failed_files);
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
