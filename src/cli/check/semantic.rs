//! Cache preflight, live-review authorization and bounded-round ownership.

use std::path::Path;

use anyhow::Result;

use super::CheckArgs;
use super::input::Work;
use super::review_budget::{Budget, Claim, Reservation};
use crate::analysis::code_quality::CodeQualityAnalyzer;
use crate::analysis::result::{AnalysisResult, FailureReason};
use crate::config::site::Refusal;

/// Whether this invocation may turn cache misses into fresh semantic reviews.
///
/// Keeping the reservation inside the state makes it impossible to run a
/// bounded live review without also owning the slot that review may consume.
pub(super) enum LiveReview {
    Skip,
    Unbounded,
    Reserved(Reservation),
    Denied { completed: u32, limit: u32 },
}

/// The semantic leg either stops after cache preflight so a push gate can
/// inspect deterministic eligibility, or completes its permitted live pass.
pub(super) enum Stage {
    Deferred(AnalysisResult),
    Complete(Box<Pass>),
}

/// Everything needed to adjudicate and account for one semantic pass after
/// the deterministic leg has supplied its compiler evidence.
pub(super) struct Pass {
    pub(super) cached: AnalysisResult,
    pub(super) live: AnalysisResult,
    pub(super) live_review: LiveReview,
    pub(super) budget: Option<Budget>,
    pub(super) should_review_live: bool,
    pub(super) limit_reached: bool,
    pub(super) live_answered: bool,
}

#[derive(Clone, Copy)]
pub(super) struct Policy {
    pub(super) authoritative: bool,
    pub(super) limit: u32,
}

/// Turn cache misses into a live semantic pass when this input is eligible.
///
/// Push gates call this only after deterministic tools pass. Other checks run
/// it inside the semantic leg of `tokio::join!`, preserving the latency overlap
/// between tools and model review.
pub(super) async fn complete(
    args: &CheckArgs,
    root: &Path,
    work: &Work,
    analyzer: &CodeQualityAnalyzer,
    policy: Policy,
    mut cached: AnalysisResult,
    allow_live: bool,
) -> Result<Pass> {
    let misses: Vec<&[_]> = work
        .by_file
        .iter()
        .filter(|hunks| {
            hunks.first().is_some_and(|first| {
                matches!(
                    cached.failed_files.get(&first.file_path),
                    Some(FailureReason::CacheMiss)
                )
            })
        })
        .map(Vec::as_slice)
        .collect();
    let should_review_live = allow_live && !args.cache_only && !misses.is_empty();
    let served_before_live: usize = analyzer
        .chain()
        .providers()
        .iter()
        .map(|provider| provider.served())
        .sum();
    let mut budget = None;
    let live_review = if !should_review_live {
        LiveReview::Skip
    } else if !policy.authoritative || args.unlimited_reviews {
        LiveReview::Unbounded
    } else {
        let resolved = Budget::for_repo(root, policy.limit).await?;
        let claim = resolved.claim()?;
        budget = Some(resolved);
        match claim {
            Claim::Reserved(reservation) => LiveReview::Reserved(reservation),
            Claim::LimitReached { completed, limit } => LiveReview::Denied { completed, limit },
        }
    };
    let limit_reached = matches!(live_review, LiveReview::Denied { .. });

    let mut live = AnalysisResult::default();
    match &live_review {
        LiveReview::Unbounded | LiveReview::Reserved(_) => {
            for hunks in &misses {
                if let Some(first) = hunks.first() {
                    cached.failed_files.remove(&first.file_path);
                }
            }
            live = analyzer.analyze_files_live(&misses).await;
        }
        LiveReview::Denied { completed, limit } => {
            for hunks in &misses {
                if let Some(first) = hunks.first() {
                    cached.failed_files.insert(
                        first.file_path.clone(),
                        FailureReason::ReviewLimit {
                            completed: *completed,
                            limit: *limit,
                        },
                    );
                }
            }
        }
        LiveReview::Skip => {}
    }
    let served_after_live: usize = analyzer
        .chain()
        .providers()
        .iter()
        .map(|provider| provider.served())
        .sum();

    Ok(Pass {
        cached,
        live,
        live_review,
        budget,
        should_review_live,
        limit_reached,
        live_answered: fresh_answered(served_before_live, served_after_live),
    })
}

fn fresh_answered(before: usize, after: usize) -> bool {
    after > before
}

/// The pass a repository gets when site policy refuses semantic review.
///
/// Every file drep was asked about is recorded as unanalyzed, one entry each,
/// exactly as a dead endpoint already renders. The alternative - one run-level
/// line - would be a second reporting mechanism outside the `unanalyzed`
/// contract, and a consumer would have to learn both.
///
/// Built here because [`Pass`] is this module's type and its fields are
/// `pub(super)`, and the fields it leaves at their inert values are what carry
/// the invariants: no reservation is claimed, no round is consumed, and
/// `should_review_live = false` keeps a refused run structurally unable to reach
/// the exit-3 push handshake. The reset guard in `run_against` cannot fire
/// either, because `failed_files` is not empty.
pub(super) fn refused(work: &Work, refusal: &Refusal) -> Pass {
    let mut cached = AnalysisResult::default();
    for hunks in &work.by_file {
        // Keyed off the first hunk, the way `complete` identifies its misses:
        // every hunk in a `by_file` entry shares a path.
        if let Some(first) = hunks.first() {
            cached.failed_files.insert(
                first.file_path.clone(),
                FailureReason::SitePolicyRefused {
                    marker: refusal.marker.clone(),
                    policy: refusal.policy.clone(),
                },
            );
        }
    }
    Pass {
        cached,
        live: AnalysisResult::default(),
        live_review: LiveReview::Skip,
        budget: None,
        should_review_live: false,
        limit_reached: false,
        live_answered: false,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn fresh_answer_requires_the_served_count_to_increase() {
        assert!(super::fresh_answered(2, 3));
        assert!(!super::fresh_answered(2, 2));
        assert!(!super::fresh_answered(3, 2));
    }
}
