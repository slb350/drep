//! The failover report: who served the run, and what a chain failure looks
//! like in both output formats.
//!
//! Rendering-level rather than end-to-end. Driving a real two-provider `check`
//! would prove the same thing more slowly and with a mock server per provider;
//! what these pin is the contract the renderer owes a user reading the terminal
//! and a consumer reading the JSON.

use super::support::{outcome, outcome_failing, provider_use, rendered, rendered_json};
use crate::analysis::result::{FailureReason, ProviderFailure};
use crate::cli::OutputFormat;
use crate::cli::check::{CheckOutcome, ProviderUse};

/// An outcome whose only content is which providers served.
fn outcome_serving(uses: Vec<ProviderUse>) -> CheckOutcome {
    CheckOutcome {
        provider_uses: uses,
        ..outcome()
    }
}

/// A run served entirely by the head prints nothing about providers.
///
/// The happy path is every run on a healthy machine. A line reporting it on
/// every commit is noise, and noise is what trains a user to stop reading the
/// block that matters.
#[test]
fn a_run_served_by_the_head_prints_no_provider_block() {
    let outcome = outcome_serving(vec![provider_use(
        0,
        "local",
        "http://localhost:1234/v1",
        4,
    )]);
    let text = rendered(&outcome, OutputFormat::Text);
    assert_eq!(
        text, "No issues found.\n",
        "a run that never failed over must print exactly the clean message"
    );
}

/// A run that fell through says so, naming the model, the endpoint and the
/// file count for every provider that served.
///
/// The head's line is printed too. "12 files went to the paid endpoint" is only
/// actionable next to "4 went to the local one".
#[test]
fn a_run_that_fell_through_names_every_provider_that_served() {
    let outcome = outcome_serving(vec![
        provider_use(0, "local-model", "http://localhost:1234/v1", 4),
        provider_use(1, "cloud-model", "https://api.example/v1", 12),
    ]);
    let text = rendered(&outcome, OutputFormat::Text);

    assert!(
        text.contains("fell through"),
        "the fallback must be announced, not silent:\n{text}"
    );
    assert!(
        text.contains("2. cloud-model at https://api.example/v1: 12 file(s)"),
        "the fallback's line must name model, endpoint and count:\n{text}"
    );
    assert!(
        text.contains("1. local-model at http://localhost:1234/v1: 4 file(s)"),
        "the head's line gives the fallback's count its context:\n{text}"
    );
}

/// The JSON `providers` array is present even on a run that never failed over.
///
/// Unlike the text block, which stays silent there. A machine consumer has no
/// noise problem, and an always-present field is what lets one distinguish "no
/// failover" from "this build of drep does not report it".
#[test]
fn the_json_providers_array_is_present_on_a_single_provider_run() {
    let outcome = outcome_serving(vec![provider_use(
        0,
        "local",
        "http://localhost:1234/v1",
        4,
    )]);
    let parsed = rendered_json(&outcome);
    let providers = parsed["providers"].as_array().expect("providers array");
    assert_eq!(providers.len(), 1);
    assert_eq!(providers[0]["provider"], 1, "one-based, like `doctor`");
    assert_eq!(providers[0]["model"], "local");
    assert_eq!(providers[0]["endpoint"], "http://localhost:1234/v1");
    assert_eq!(providers[0]["files"], 4);
}

/// A run with no LLM work at all still carries the (empty) array.
#[test]
fn the_json_providers_array_is_present_and_empty_when_nothing_was_served() {
    let outcome = outcome();
    let parsed = rendered_json(&outcome);
    assert_eq!(
        parsed["providers"]
            .as_array()
            .expect("providers array")
            .len(),
        0
    );
}

fn chain_failure() -> FailureReason {
    FailureReason::ChainFailed(vec![
        ProviderFailure {
            provider: 0,
            model: "local-model".to_owned(),
            reason: FailureReason::Transport {
                status: None,
                message: "connection refused".to_owned(),
            },
            skipped: true,
        },
        ProviderFailure {
            provider: 1,
            model: "cloud-model".to_owned(),
            reason: FailureReason::Transport {
                status: Some(401),
                message: "unauthorized".to_owned(),
            },
            skipped: false,
        },
    ])
}

/// The text line for a chain failure names every provider and what each said.
///
/// Reporting only the last would hide the dead local endpoint behind the
/// cloud's 401; reporting only the first would hide the bad key. A user fixing
/// this run needs both, and needs to know the local one has been down since
/// earlier in the run rather than having just failed.
#[test]
fn a_chain_failure_reports_every_provider_in_the_text_output() {
    let outcome = outcome_failing(vec![("src/lib.rs", chain_failure())]);
    let text = rendered(&outcome, OutputFormat::Text);

    assert!(
        text.contains("no LLM provider analyzed this file"),
        "{text}"
    );
    assert!(text.contains("[1] local-model"), "{text}");
    assert!(text.contains("connection refused"), "{text}");
    assert!(
        text.contains("already down earlier in this run"),
        "a skipped provider must be distinguishable from one just contacted:\n{text}"
    );
    assert!(text.contains("[2] cloud-model"), "{text}");
    assert!(text.contains("401"), "{text}");
}

/// The JSON carries the per-provider detail as structure, not prose.
#[test]
fn a_chain_failure_carries_structured_per_provider_detail_in_json() {
    let outcome = outcome_failing(vec![("src/lib.rs", chain_failure())]);
    let parsed = rendered_json(&outcome);
    let entry = &parsed["unanalyzed"][0];

    assert_eq!(entry["kind"], "chain_failed");
    assert!(
        entry.get("status").is_none(),
        "a chain failure has one status per provider; flattening them would \
         claim the last provider's code was the whole story"
    );

    let providers = entry["providers"].as_array().expect("providers array");
    assert_eq!(providers.len(), 2);
    assert_eq!(providers[0]["provider"], 1, "one-based");
    assert_eq!(providers[0]["model"], "local-model");
    assert_eq!(providers[0]["kind"], "transport");
    assert_eq!(providers[0]["skipped"], true);
    assert!(
        providers[0].get("status").is_none(),
        "a refused connection has no HTTP status"
    );
    assert_eq!(providers[1]["provider"], 2);
    assert_eq!(providers[1]["status"], 401);
    assert_eq!(providers[1]["skipped"], false);
}

/// A single failing provider keeps the pre-failover shape, `kind` included.
///
/// This is what makes failover invisible to the config `drep init` writes. A
/// chain error that always wrapped, even around one provider, would change the
/// JSON `kind` of every single-provider run from `transport` to `all_providers`
/// and break a consumer that never asked for failover.
#[test]
fn a_single_provider_failure_still_renders_as_a_plain_transport_failure() {
    let outcome = outcome_failing(vec![(
        "src/lib.rs",
        FailureReason::Transport {
            status: Some(500),
            message: "internal".to_owned(),
        },
    )]);
    let parsed = rendered_json(&outcome);
    let entry = &parsed["unanalyzed"][0];
    assert_eq!(entry["kind"], "transport");
    assert_eq!(entry["status"], 500);
    assert!(entry.get("providers").is_none());
}

/// `provider_uses` reports the providers that answered, and only those.
///
/// Everything else in this file builds `ProviderUse` by hand, which pins the
/// rendering but leaves the projection off the chain untested — and the filter
/// is exactly where it can silently invert, reporting the untouched fallback
/// and omitting the model that actually reviewed the code.
#[tokio::test]
async fn provider_uses_names_the_providers_that_answered_and_no_others() {
    use crate::llm::chain::ProviderChain;
    use crate::test_support::{
        cfg_for, fast_retry_chain, server_failing_with, server_returning, temp_cache,
    };

    let dead = server_failing_with(500).await;
    let healthy = server_returning(&[r#"{"issues": [], "summary": "ok"}"#]).await;
    let (cache, _dir) = temp_cache();
    let chain: ProviderChain = fast_retry_chain(&[
        cfg_for(&dead, "model-a", 1),
        cfg_for(&healthy, "model-b", 1),
    ]);

    chain
        .complete_json("system", "content", &cache)
        .await
        .expect("the fallback answers");

    let uses = super::super::provider_uses(&chain);
    assert_eq!(
        uses.len(),
        1,
        "only the provider that answered is reported, got {:?}",
        uses.iter().map(|u| u.index).collect::<Vec<_>>()
    );
    assert_eq!(uses[0].index, 1, "the fallback answered, not the head");
    assert_eq!(uses[0].model, "model-b");
    assert_eq!(uses[0].files, 1);
}
