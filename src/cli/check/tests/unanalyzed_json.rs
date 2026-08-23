//! The machine-readable shape of `--format json`'s `unanalyzed` array.
//!
//! Every entry used to carry only `file` and `reason`, where `reason` is an
//! English sentence. A consumer wanting to tell "the endpoint rate-limited us"
//! from "the endpoint is misconfigured" had to match on that prose. Failover
//! makes exactly that call because a 429 should advance and a 401 must not.
//! Each entry therefore carries a stable `kind` tag, plus an HTTP `status` or
//! process `backend_kind` where one exists.

use serde_json::Value;

use super::support::{outcome, outcome_failing, rendered_json};
use crate::analysis::result::{FailureReason, ProviderFailure};
use crate::cli::check::ReviewActivity;
use crate::llm::error::BackendErrorKind;

/// Render `failures` as JSON and hand back the parsed `unanalyzed` array.
fn unanalyzed_for(failures: Vec<(&str, FailureReason)>) -> Vec<Value> {
    let parsed = rendered_json(&outcome_failing(failures));
    parsed["unanalyzed"]
        .as_array()
        .expect("unanalyzed is an array")
        .clone()
}

/// The tag each variant is expected to render, restated here independently of
/// production.
///
/// Written as an **exhaustive match** on purpose. The sample list below is
/// hand-written, and a new `FailureReason` variant added without a sample would
/// otherwise slip through with no tag test at all - which is exactly what
/// happened when `AllProviders` arrived and this file kept passing. Adding a
/// variant now fails to compile here, in the file holding the list.
fn expected_kind(reason: &FailureReason) -> &'static str {
    match reason {
        FailureReason::Transport { .. } => "transport",
        FailureReason::Backend { .. } => "backend",
        FailureReason::Unparseable(_) => "unparseable",
        FailureReason::CacheMiss => "cache_miss",
        FailureReason::ReviewLimit { .. } => "review_limit",
        FailureReason::ModelStopped { .. } => "model_stopped",
        FailureReason::Truncated => "truncated",
        FailureReason::MalformedFinding(_) => "malformed_finding",
        FailureReason::ToolUnavailable { .. } => "tool_unavailable",
        FailureReason::FileTooLarge { .. } => "file_too_large",
        FailureReason::PayloadTooLarge { .. } => "payload_too_large",
        FailureReason::Unreadable(_) => "unreadable",
        FailureReason::Unsupported { .. } => "unsupported",
        FailureReason::ChainFailed(_) => "chain_failed",
    }
}

/// Every `FailureReason` variant renders a distinct `kind`.
///
/// Listed exhaustively rather than sampled: the tags are what a consumer
/// branches on, so two variants collapsing to one tag is a silent loss of
/// information exactly where the JSON format is supposed to be adding it.
#[test]
fn each_failure_variant_renders_its_own_kind_tag() {
    let cases: Vec<FailureReason> = vec![
        FailureReason::Transport {
            status: Some(500),
            message: "boom".to_owned(),
        },
        FailureReason::Backend {
            kind: BackendErrorKind::Contract,
            message: "tool event".to_owned(),
        },
        FailureReason::Unparseable("no json".to_owned()),
        FailureReason::CacheMiss,
        FailureReason::ReviewLimit {
            completed: 3,
            limit: 3,
        },
        FailureReason::ModelStopped {
            finish: "length".to_owned(),
            message: "the model hit its output token limit".to_owned(),
        },
        FailureReason::Truncated,
        FailureReason::MalformedFinding("bad severity".to_owned()),
        FailureReason::ToolUnavailable {
            tool: "ruff".to_owned(),
            detail: "not found".to_owned(),
        },
        FailureReason::FileTooLarge { bytes: 1, limit: 0 },
        FailureReason::PayloadTooLarge { bytes: 1, limit: 0 },
        FailureReason::Unreadable("eperm".to_owned()),
        FailureReason::Unsupported {
            extension: Some(".md".to_owned()),
            hint: Some("run `drep lint-docs` instead".to_owned()),
        },
        FailureReason::ChainFailed(vec![ProviderFailure {
            provider: 0,
            model: "m".to_owned(),
            reason: FailureReason::Transport {
                status: None,
                message: "refused".to_owned(),
            },
            skipped: false,
        }]),
    ];

    let mut seen: Vec<&str> = Vec::new();
    for reason in cases {
        let expected = expected_kind(&reason);
        let entries = unanalyzed_for(vec![("src/a.rs", reason.clone())]);
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0]["kind"].as_str(),
            Some(expected),
            "{reason:?} must render kind {expected}"
        );
        seen.push(expected);
    }
    let mut unique = seen.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(
        unique.len(),
        seen.len(),
        "every variant needs its own tag; duplicates found in {seen:?}"
    );
}

#[test]
fn review_activity_has_a_stable_machine_readable_shape() {
    let cases = [
        (
            ReviewActivity::Counted { round: 2, limit: 3 },
            serde_json::json!({"kind": "counted", "round": 2, "limit": 3}),
        ),
        (ReviewActivity::Reset, serde_json::json!({"kind": "reset"})),
        (
            ReviewActivity::Unlimited,
            serde_json::json!({"kind": "unlimited"}),
        ),
    ];

    for (activity, expected) in cases {
        let mut check = outcome();
        check.review_activity = Some(activity);
        assert_eq!(rendered_json(&check)["review"], expected);
    }
    assert!(rendered_json(&outcome())["review"].is_null());
}

#[test]
fn a_backend_failure_exposes_its_typed_class_without_prose_matching() {
    let entries = unanalyzed_for(vec![(
        "src/a.rs",
        FailureReason::Backend {
            kind: BackendErrorKind::UnknownExit,
            message: "unauthorized quota timeout words are not a classifier".to_owned(),
        },
    )]);

    assert_eq!(entries[0]["kind"].as_str(), Some("backend"));
    assert_eq!(entries[0]["backend_kind"].as_str(), Some("unknown_exit"));
    assert!(entries[0].get("status").is_none());
}

/// A transport failure with an HTTP code exposes it as a **number**.
///
/// The number is the point: 5c branches on 429 versus 401, and a consumer
/// substring-matching the prose would break the first time the message is
/// reworded.
#[test]
fn a_transport_failure_exposes_its_status_as_a_number() {
    let entries = unanalyzed_for(vec![(
        "src/a.rs",
        FailureReason::Transport {
            status: Some(429),
            message: "rate limited".to_owned(),
        },
    )]);
    assert_eq!(entries[0]["status"].as_u64(), Some(429));
    assert_eq!(entries[0]["kind"].as_str(), Some("transport"));
    assert_eq!(entries[0]["file"].as_str(), Some("src/a.rs"));
    assert!(
        entries[0]["reason"]
            .as_str()
            .expect("reason is a string")
            .contains("429"),
        "the human line keeps carrying the code too"
    );
}

/// A failure with no HTTP code omits `status` entirely rather than emitting
/// `null`, so the key's presence is itself the signal.
#[test]
fn a_failure_without_a_status_omits_the_key_rather_than_nulling_it() {
    for reason in [
        FailureReason::Truncated,
        FailureReason::Transport {
            status: None,
            message: "connection refused".to_owned(),
        },
    ] {
        let entries = unanalyzed_for(vec![("src/a.rs", reason.clone())]);
        let obj = entries[0].as_object().expect("entry is an object");
        assert!(
            !obj.contains_key("status"),
            "{reason:?} has no HTTP status, so `status` must be absent, got {obj:?}"
        );
    }
}

#[test]
fn review_limit_exposes_structured_progress_and_recovery_text() {
    let entries = unanalyzed_for(vec![(
        "src/a.rs",
        FailureReason::ReviewLimit {
            completed: 3,
            limit: 3,
        },
    )]);

    assert_eq!(entries[0]["kind"].as_str(), Some("review_limit"));
    assert_eq!(entries[0]["completed"].as_u64(), Some(3));
    assert_eq!(entries[0]["limit"].as_u64(), Some(3));
    assert!(
        entries[0]["reason"]
            .as_str()
            .expect("reason")
            .contains("--max-review-rounds")
    );
}

#[test]
fn review_limit_distinguishes_in_flight_reservations_from_completed_rounds() {
    let entries = unanalyzed_for(vec![(
        "src/a.rs",
        FailureReason::ReviewLimit {
            completed: 1,
            limit: 3,
        },
    )]);

    assert_eq!(entries[0]["completed"].as_u64(), Some(1));
    assert!(
        entries[0]["reason"]
            .as_str()
            .expect("reason")
            .contains("currently reserved")
    );
}
