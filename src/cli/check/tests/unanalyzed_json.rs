//! The machine-readable shape of `--format json`'s `unanalyzed` array.
//!
//! Every entry used to carry only `file` and `reason`, where `reason` is an
//! English sentence. A consumer wanting to tell "the endpoint rate-limited us"
//! from "the endpoint is misconfigured" had to match on that prose — and Phase
//! 5c's failover has to make exactly that call, because a 429 should fail over
//! to the next provider and a 401 must not (falling back would mask the
//! misconfiguration). So each entry now carries a stable `kind` tag, and an
//! HTTP `status` when there was one.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::Value;

use crate::Exit;
use crate::analysis::result::FailureReason;
use crate::cli::OutputFormat;
use crate::cli::check::{CheckOutcome, render};

/// Render `failures` as JSON and hand back the parsed `unanalyzed` array.
fn unanalyzed_for(failures: Vec<(&str, FailureReason)>) -> Vec<Value> {
    let map: BTreeMap<PathBuf, FailureReason> = failures
        .into_iter()
        .map(|(path, reason)| (PathBuf::from(path), reason))
        .collect();
    let outcome = CheckOutcome {
        tool_findings: Vec::new(),
        llm_findings: Vec::new(),
        failures: map,
        exit: Exit::Unanalyzed,
    };
    let mut buf: Vec<u8> = Vec::new();
    render::render_to(&mut buf, &outcome, OutputFormat::Json).expect("render");
    let parsed: Value = serde_json::from_slice(&buf).expect("valid JSON");
    parsed["unanalyzed"]
        .as_array()
        .expect("unanalyzed is an array")
        .clone()
}

/// Every `FailureReason` variant renders a distinct `kind`.
///
/// Listed exhaustively rather than sampled: the tags are what a consumer
/// branches on, so two variants collapsing to one tag is a silent loss of
/// information exactly where the JSON format is supposed to be adding it.
#[test]
fn each_failure_variant_renders_its_own_kind_tag() {
    let cases: Vec<(FailureReason, &str)> = vec![
        (
            FailureReason::Transport {
                status: Some(500),
                message: "boom".to_owned(),
            },
            "transport",
        ),
        (
            FailureReason::Unparseable("no json".to_owned()),
            "unparseable",
        ),
        (FailureReason::Truncated, "truncated"),
        (
            FailureReason::MalformedFinding("bad severity".to_owned()),
            "malformed_finding",
        ),
        (
            FailureReason::ToolUnavailable {
                tool: "ruff".to_owned(),
                detail: "not found".to_owned(),
            },
            "tool_unavailable",
        ),
        (
            FailureReason::FileTooLarge { bytes: 1, limit: 0 },
            "file_too_large",
        ),
        (
            FailureReason::PayloadTooLarge { bytes: 1, limit: 0 },
            "payload_too_large",
        ),
        (FailureReason::Unreadable("eperm".to_owned()), "unreadable"),
    ];

    let mut seen: Vec<&str> = Vec::new();
    for (reason, expected) in cases {
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
