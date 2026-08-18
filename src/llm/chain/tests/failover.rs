//! What advances the chain, and what stops it dead.
//!
//! Every test here asserts on the *second* server's request count as well as
//! the outcome. "Provider B answered" and "provider B was never asked" are the
//! two halves of the policy, and a test checking only the returned value
//! cannot tell a chain that correctly refused to fail over from one that
//! failed over and happened to get the same error back.

use super::support::{
    CONTENT, SYSTEM, server_returning_json, server_returning_nothing, server_returning_prose,
};
use crate::llm::json_parsing::Extracted;
use crate::test_support::{
    cfg_for, fast_retry_chain, request_count, server_failing_with, temp_cache,
};

/// A retryable 5xx hands the file to the next provider.
#[tokio::test]
async fn a_5xx_fails_over_to_the_next_provider() {
    let dead = server_failing_with(500).await;
    let healthy = server_returning_json().await;
    let (cache, _dir) = temp_cache();
    let chain = fast_retry_chain(&[cfg_for(&dead, "a", 1), cfg_for(&healthy, "b", 1)]);

    let served = chain
        .complete_json(SYSTEM, CONTENT, &cache)
        .await
        .expect("provider 2 answers");

    assert_eq!(served.provider, 1, "the second provider served the file");
    assert!(!served.from_cache);
    assert!(matches!(served.extracted, Extracted::Complete(_)));
    assert_eq!(request_count(&dead).await, 1, "provider 1 was tried");
    assert_eq!(
        request_count(&healthy).await,
        1,
        "provider 2 was tried once"
    );
}

/// A 429 fails over. It is a 4xx, so a naive "only 5xx" rule would stop here -
/// and rate limiting is one of the two cases failover exists for.
#[tokio::test]
async fn a_429_fails_over() {
    let limited = server_failing_with(429).await;
    let healthy = server_returning_json().await;
    let (cache, _dir) = temp_cache();
    let chain = fast_retry_chain(&[cfg_for(&limited, "a", 1), cfg_for(&healthy, "b", 1)]);

    let served = chain
        .complete_json(SYSTEM, CONTENT, &cache)
        .await
        .expect("provider 2 answers");
    assert_eq!(served.provider, 1);
}

/// A 401 must NOT fail over. It is misconfiguration, and asking a second
/// provider hides it behind a working answer.
#[tokio::test]
async fn a_401_does_not_fail_over_and_the_next_provider_is_never_asked() {
    let unauthorized = server_failing_with(401).await;
    let healthy = server_returning_json().await;
    let (cache, _dir) = temp_cache();
    let chain = fast_retry_chain(&[cfg_for(&unauthorized, "a", 1), cfg_for(&healthy, "b", 1)]);

    let err = chain
        .complete_json(SYSTEM, CONTENT, &cache)
        .await
        .expect_err("a 401 is fatal for the file");

    assert_eq!(
        err.attempts.len(),
        1,
        "only the provider that actually failed is reported, got {:?}",
        err.attempts
    );
    assert_eq!(err.attempts[0].provider, 0);
    assert_eq!(err.attempts[0].error.status(), Some(401));
    assert_eq!(
        request_count(&healthy).await,
        0,
        "the fallback must never see a request when the failure was a 401"
    );
    assert!(
        chain.providers()[0].is_down(),
        "a rejected credential is a property of the endpoint, not of this file - \
         it is remembered so the rest of the run does not re-handshake to be told again"
    );
}

/// A 403 behaves like a 401: still misconfiguration, still no failover.
#[tokio::test]
async fn a_403_does_not_fail_over() {
    let forbidden = server_failing_with(403).await;
    let healthy = server_returning_json().await;
    let (cache, _dir) = temp_cache();
    let chain = fast_retry_chain(&[cfg_for(&forbidden, "a", 1), cfg_for(&healthy, "b", 1)]);

    chain
        .complete_json(SYSTEM, CONTENT, &cache)
        .await
        .expect_err("a 403 is fatal for the file");
    assert_eq!(request_count(&healthy).await, 0);
}

/// An empty response fails over.
///
/// The decision recorded in `docs/rust-migration.md`: zero characters means
/// the provider did not answer, it already cost the SDK's retries, and it
/// produced no output tokens - so the next provider is the first one likely to
/// actually answer.
#[tokio::test]
async fn an_empty_response_fails_over() {
    let silent = server_returning_nothing().await;
    let healthy = server_returning_json().await;
    let (cache, _dir) = temp_cache();
    let chain = fast_retry_chain(&[cfg_for(&silent, "a", 1), cfg_for(&healthy, "b", 1)]);

    let served = chain
        .complete_json(SYSTEM, CONTENT, &cache)
        .await
        .expect("provider 2 answers");
    assert_eq!(served.provider, 1);
    assert_eq!(
        request_count(&healthy).await,
        1,
        "the empty body must have handed the file on"
    );
}

/// A non-empty body carrying no JSON must NOT fail over.
///
/// This is the deterministic case: the same prompt produces the same
/// unparseable answer, so a second provider is a second full-price call for
/// the same outcome. It is also the discriminating counterpart to the test
/// above - a rule that failed over on "no JSON extracted" would pass the
/// empty-body test and be wrong here.
#[tokio::test]
async fn an_unparseable_but_non_empty_response_does_not_fail_over() {
    let chatty = server_returning_prose().await;
    let healthy = server_returning_json().await;
    let (cache, _dir) = temp_cache();
    let chain = fast_retry_chain(&[cfg_for(&chatty, "a", 1), cfg_for(&healthy, "b", 1)]);

    let err = chain
        .complete_json(SYSTEM, CONTENT, &cache)
        .await
        .expect_err("prose with no JSON fails the file");
    assert_eq!(err.attempts.len(), 1);
    assert_eq!(
        request_count(&healthy).await,
        0,
        "a deterministic parse failure must not spend a second provider"
    );
    assert!(
        !chain.providers()[0].is_down(),
        "an unparseable answer is about this payload, not the endpoint - the next \
         file's might parse, so the provider is not written off"
    );
}

/// A remembered 401 still stops the chain, on every later file.
///
/// The discriminating case for splitting "remember it" from "fail over on it".
/// Marking a 401 down without replaying it through the failover rule would make
/// every file after the first *skip* the head and be served happily by the
/// fallback - so a stale key would be silently routed around, which is the one
/// thing the 401 rule exists to prevent.
#[tokio::test]
async fn a_remembered_401_still_stops_the_chain_on_later_files() {
    let unauthorized = server_failing_with(401).await;
    let healthy = server_returning_json().await;
    let (cache, _dir) = temp_cache();
    let chain = fast_retry_chain(&[cfg_for(&unauthorized, "a", 1), cfg_for(&healthy, "b", 1)]);

    chain
        .complete_json(SYSTEM, "file one", &cache)
        .await
        .expect_err("the 401 fails the first file");

    let err = chain
        .complete_json(SYSTEM, "file two", &cache)
        .await
        .expect_err("and every file after it");

    assert_eq!(err.attempts.len(), 1, "the chain still stops at the head");
    assert!(
        err.attempts[0].skipped,
        "the head was not contacted again for this file"
    );
    assert_eq!(err.attempts[0].error.status(), Some(401));
    assert_eq!(
        request_count(&unauthorized).await,
        1,
        "one handshake for the whole run, not one per file"
    );
    assert_eq!(
        request_count(&healthy).await,
        0,
        "a remembered 401 must not start failing over"
    );
}

/// A healthy head is used, and the tail is never contacted.
#[tokio::test]
async fn a_healthy_head_serves_and_the_tail_is_untouched() {
    let healthy = server_returning_json().await;
    let fallback = server_returning_json().await;
    let (cache, _dir) = temp_cache();
    let chain = fast_retry_chain(&[cfg_for(&healthy, "a", 1), cfg_for(&fallback, "b", 1)]);

    let served = chain
        .complete_json(SYSTEM, CONTENT, &cache)
        .await
        .expect("provider 1 answers");
    assert_eq!(served.provider, 0);
    assert_eq!(request_count(&fallback).await, 0);
}

/// When every provider fails, every provider's reason is reported.
///
/// Keeping only the last one would hide the dead local endpoint behind the
/// cloud fallback's error, and keeping only the first would hide the
/// misconfigured fallback. The user needs both to fix the run.
#[tokio::test]
async fn when_all_providers_fail_each_reason_is_reported() {
    let first = server_failing_with(500).await;
    let second = server_failing_with(503).await;
    let (cache, _dir) = temp_cache();
    let chain = fast_retry_chain(&[cfg_for(&first, "a", 1), cfg_for(&second, "b", 1)]);

    let err = chain
        .complete_json(SYSTEM, CONTENT, &cache)
        .await
        .expect_err("both providers failed");

    assert_eq!(err.attempts.len(), 2);
    assert_eq!(err.attempts[0].error.status(), Some(500));
    assert_eq!(err.attempts[0].model, "a");
    assert!(!err.attempts[0].skipped);
    assert_eq!(err.attempts[1].error.status(), Some(503));
    assert_eq!(err.attempts[1].model, "b");
    assert!(!err.attempts[1].skipped);
}
