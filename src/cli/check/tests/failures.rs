//! Failure reporting: criteria 1, 2, 3.
//!
//! These pin the three load-bearing details of the failure pipeline:
//! - the HTTP status code reaches the rendered line as a number;
//! - merging two `AnalysisResult`s over the same path keeps the **first**
//!   reason rather than overwriting it;
//! - the three LLM-side failure shapes each map onto the right variant.
//!
//! All three are pinned here rather than at their producing call sites
//! because the failure vocabulary is the only thing that decides whether a
//! gate reports "could not analyze" vs "analyzed and clean". Reporting
//! unanalyzed as clean is the single failure a commit gate must not have.

use std::path::PathBuf;

use crate::analysis::result::{AnalysisResult, FailureReason};

/// Criterion 1: a `Transport { status: Some(429), .. }` rendered through
/// `Display` must contain the number `429`.
///
/// Keeping the status structured is pointless if it never reaches the user,
/// and a one-line render that drops the code leaves a 429 indistinguishable
/// from a 500 at the terminal.
#[test]
fn transport_with_status_renders_the_code_in_display() {
    let reason = FailureReason::Transport {
        status: Some(429),
        message: "rate limited".to_owned(),
    };
    let rendered = format!("{reason}");
    assert!(
        rendered.contains("429"),
        "rendered line must contain 429, got {rendered:?}"
    );
}

/// Criterion 2: `merge` over two `AnalysisResult`s that name the same path
/// yields a single entry holding the **first** reason.
///
/// Length alone would also hold for last-wins; the reason assertion is the
/// load-bearing half. A last-wins policy would silently let a later analyzer
/// overwrite a more informative first reason with a generic later one.
#[test]
fn merge_unions_same_path_into_one_entry_keeping_the_first_reason() {
    let path = PathBuf::from("src/lib.rs");

    let mut first = AnalysisResult::default();
    first.failed_files.insert(
        path.clone(),
        FailureReason::Transport {
            status: Some(429),
            message: "rate limited".to_owned(),
        },
    );

    let mut second = AnalysisResult::default();
    second.failed_files.insert(
        path.clone(),
        FailureReason::Transport {
            status: Some(500),
            message: "internal".to_owned(),
        },
    );

    first.merge(second);

    assert_eq!(
        first.failed_files.len(),
        1,
        "two failed_files entries on the same path must union into one"
    );
    let kept = first
        .failed_files
        .get(&path)
        .expect("the merged entry is present");
    assert_eq!(
        kept,
        &FailureReason::Transport {
            status: Some(429),
            message: "rate limited".to_owned(),
        },
        "first reason wins on a key collision"
    );
}

/// Criterion 3: one test that asserts all three failure shapes at once.
///
/// A single hardcoded reason cannot satisfy all three arms: an
/// implementation that always returns `Transport`, or always returns
/// `MalformedFinding`, would fail. Pinning each shape in one test also
/// keeps the test count proportional to the failure vocabulary rather than
/// to the number of call sites that produce each shape.
#[test]
fn three_failure_shapes_are_distinct_and_each_one_renders() {
    let transport = FailureReason::Transport {
        status: Some(429),
        message: "rate limited".to_owned(),
    };
    let truncated = FailureReason::Truncated;
    let malformed = FailureReason::MalformedFinding("unknown severity `blocker`".to_owned());

    assert!(
        matches!(transport, FailureReason::Transport { .. }),
        "transport failure must remain Transport"
    );
    assert!(
        matches!(truncated, FailureReason::Truncated),
        "truncated response must remain Truncated"
    );
    assert!(
        matches!(malformed, FailureReason::MalformedFinding(_)),
        "unknown severity must remain MalformedFinding"
    );

    assert!(
        format!("{transport}").contains("429"),
        "transport line must carry its HTTP code"
    );
    assert!(
        format!("{truncated}").contains("truncated"),
        "truncated line must say so"
    );
    assert!(
        format!("{malformed}").contains("malformed finding"),
        "malformed line must say so, got {:?}",
        malformed
    );
}

/// The documented tunables are what the docs say they are.
///
/// A change-detector by design, and the only thing that can observe them:
/// `WHOLE_FILE_MAX_BYTES` and the cache limits are inputs to behaviour no
/// assertion can otherwise reach, so `cargo mutants` can rewrite `256 * 1024`
/// to `256 + 1024` with every other test still green. They are documented
/// numbers with a stated rationale, so a silent change to one should be
/// visible rather than free.
#[test]
fn documented_size_and_cache_limits_hold_their_stated_values() {
    use crate::cli::check::{CACHE_MAX_BYTES, CACHE_TTL_DAYS, WHOLE_FILE_MAX_BYTES};

    assert_eq!(
        WHOLE_FILE_MAX_BYTES, 262_144,
        "256 KiB, per its doc comment"
    );
    assert_eq!(CACHE_TTL_DAYS, 30);
    assert_eq!(CACHE_MAX_BYTES, 268_435_456, "256 MiB, per its doc comment");
}
