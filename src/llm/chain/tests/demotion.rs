//! Sticky demotion: a provider that failed over stays skipped for the run.
//!
//! The motivating case is a local endpoint that is off with forty-nine changed
//! files. Without demotion each file pays the SDK's full retry schedule
//! against a socket nobody is listening on, for a verdict already known.

use super::support::{SYSTEM, server_returning_json, server_returning_prose};
use crate::test_support::{
    cfg_for, fast_retry_chain, request_count, server_failing_with, temp_cache,
};

/// The second call skips the demoted head entirely.
///
/// Asserting the request count is what makes this discriminating: a chain
/// without demotion returns exactly the same `Served` for both calls, because
/// the fallback answers either way. Only the head's request count changes.
#[tokio::test]
async fn a_demoted_provider_is_not_contacted_again() {
    let dead = server_failing_with(500).await;
    let healthy = server_returning_json().await;
    let (cache, _dir) = temp_cache();
    // Distinct payloads so the second call cannot be answered from the cache
    // the first one would have populated - this test is about the provider
    // loop, not about caching.
    let chain = fast_retry_chain(&[cfg_for(&dead, "a", 1), cfg_for(&healthy, "b", 1)]);

    let first = chain
        .complete_json(SYSTEM, "file one", &cache)
        .await
        .expect("provider 2 answers");
    assert_eq!(first.provider, 1);
    assert!(
        chain.providers()[0].is_down(),
        "the head is demoted after failing over"
    );
    assert_eq!(request_count(&dead).await, 1);

    let second = chain
        .complete_json(SYSTEM, "file two", &cache)
        .await
        .expect("provider 2 answers again");
    assert_eq!(second.provider, 1);
    assert_eq!(
        request_count(&dead).await,
        1,
        "the demoted head must not be contacted a second time"
    );
    assert_eq!(request_count(&healthy).await, 2);
}

/// A skipped provider still appears in the failure report, marked skipped,
/// carrying the reason it went down.
///
/// A later file that fails everywhere must still explain the head. Reporting
/// only the providers contacted for *this* file would say "the cloud returned
/// a 401" and never mention that the local endpoint has been dead since file
/// one.
#[tokio::test]
async fn a_skipped_provider_is_reported_with_the_reason_it_went_down() {
    let dead = server_failing_with(500).await;
    let broken = server_returning_prose().await;
    let (cache, _dir) = temp_cache();
    let chain = fast_retry_chain(&[cfg_for(&dead, "a", 1), cfg_for(&broken, "b", 1)]);

    // First file: the head fails over, the fallback answers with prose, so the
    // file fails - and the head is now demoted.
    chain
        .complete_json(SYSTEM, "file one", &cache)
        .await
        .expect_err("the fallback cannot parse");
    assert!(chain.providers()[0].is_down());

    let err = chain
        .complete_json(SYSTEM, "file two", &cache)
        .await
        .expect_err("both providers still fail");

    assert_eq!(err.attempts.len(), 2, "both providers are accounted for");
    assert!(
        err.attempts[0].skipped,
        "the head was skipped, not contacted"
    );
    assert_eq!(
        err.attempts[0].error.status(),
        Some(500),
        "the recorded reason is the one that demoted it"
    );
    assert!(!err.attempts[1].skipped);
    assert_eq!(
        request_count(&dead).await,
        1,
        "the demoted head was contacted once, on the first file"
    );
}

/// Every provider down: the report still names each one, and nobody is
/// contacted.
#[tokio::test]
async fn a_fully_demoted_chain_reports_every_provider_without_contacting_any() {
    let first = server_failing_with(500).await;
    let second = server_failing_with(503).await;
    let (cache, _dir) = temp_cache();
    let chain = fast_retry_chain(&[cfg_for(&first, "a", 1), cfg_for(&second, "b", 1)]);

    chain
        .complete_json(SYSTEM, "file one", &cache)
        .await
        .expect_err("both fail");
    assert!(chain.providers()[0].is_down() && chain.providers()[1].is_down());

    let err = chain
        .complete_json(SYSTEM, "file two", &cache)
        .await
        .expect_err("both are down");

    assert_eq!(err.attempts.len(), 2);
    assert!(err.attempts.iter().all(|a| a.skipped));
    assert_eq!(request_count(&first).await, 1);
    assert_eq!(request_count(&second).await, 1);
}

/// A demoted head does not block a cache hit for itself.
///
/// The cache lookup is inside the loop, after the down check - so a demoted
/// provider is skipped even when it has a cached answer. That is the correct
/// order: the demotion says "do not spend a request here", and a cache hit
/// spends none. This pins the behaviour deliberately rather than by accident,
/// because reversing the two lines is a one-character change with no other
/// visible effect.
#[tokio::test]
async fn a_demoted_provider_is_skipped_even_when_it_has_a_cached_answer() {
    let dead = server_failing_with(500).await;
    let healthy = server_returning_json().await;
    let (cache, _dir) = temp_cache();
    let chain = fast_retry_chain(&[
        cfg_for(&dead, "model-a", 1),
        cfg_for(&healthy, "model-b", 1),
    ]);

    chain
        .complete_json(SYSTEM, "file one", &cache)
        .await
        .expect("the fallback answers");
    assert!(chain.providers()[0].is_down());

    let planted = serde_json::json!({"issues": [], "summary": "clean"});
    cache
        .put(
            &chain.providers()[0].cache_key(&cache, SYSTEM, "file two"),
            &planted,
        )
        .expect("cache write");

    let served = chain
        .complete_json(SYSTEM, "file two", &cache)
        .await
        .expect("the fallback answers again");
    assert_eq!(
        served.provider, 1,
        "a demoted provider is skipped before its cache is consulted"
    );
}

/// A file already queued on a provider's limiter is skipped once that provider
/// goes down while it waited.
///
/// The check before the limiter can only stop files that had not started.
/// Everything already queued passed it before the first failure landed, so
/// without a second look after the slot is granted, sticky demotion saves
/// nothing in the exact case it exists for: many files, one dead endpoint.
///
/// One permit makes this deterministic rather than a race - the second call is
/// admitted only after the first has failed and marked the provider down.
#[tokio::test]
async fn a_file_waiting_on_the_limiter_is_skipped_once_the_provider_goes_down() {
    let dead = server_failing_with(500).await;
    let healthy = server_returning_json().await;
    let (cache, _dir) = temp_cache();
    let mut head = cfg_for(&dead, "a", 1);
    head.max_concurrent = 1;
    let chain = fast_retry_chain(&[head, cfg_for(&healthy, "b", 1)]);

    let (first, second) = tokio::join!(
        chain.complete_json(SYSTEM, "file one", &cache),
        chain.complete_json(SYSTEM, "file two", &cache),
    );

    assert_eq!(first.expect("served").provider, 1);
    assert_eq!(second.expect("served").provider, 1);
    assert_eq!(
        request_count(&dead).await,
        1,
        "the second file waited on the head's only permit and must be skipped, \
         not sent, once the first file demoted it"
    );
}
