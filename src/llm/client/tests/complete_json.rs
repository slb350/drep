//! `LlmClient::complete_json` - request flow against a mock endpoint.
//!
//! Criteria 22-29 plus the `sdk_classifies_400_as_non_retryable` regression
//! pin. Each test owns a `MockServer`, mounts exactly the mocks it needs,
//! and asserts on the returned `Extracted` / `LlmError` variant AND on the
//! mock's request count where the criterion depends on retry behaviour.

use open_agent::retry::is_retryable_error;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::llm::client::tests::support::{cfg_for, fast_retry_client, request_count, sse};
use crate::llm::client::{Extracted, LlmClient, LlmError};

/// Criterion 22: a 200 response with a fenced JSON body yields `Complete`.
#[tokio::test]
async fn fenced_json_response_yields_complete() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(sse(&["{\"findings\":[]}"]), "text/event-stream"),
        )
        .mount(&server)
        .await;

    let client = LlmClient::new(&cfg_for(&server, "m", 3)).expect("client");
    let extracted = client.complete_json("sys", "user").await.expect("ok");

    assert!(
        matches!(extracted, Extracted::Complete(_)),
        "fenced JSON must be Complete, got {extracted:?}"
    );
    assert_eq!(extracted, Extracted::Complete(json!({"findings": []})));
}

/// Criterion 23: a 200 response with prose and no JSON yields `Unparseable`.
/// Crucially, the spec splits parse failure from transport failure so the
/// caller can decide which to retry.
#[tokio::test]
async fn prose_without_json_yields_unparseable() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            sse(&["Here is some prose without any JSON in it."]),
            "text/event-stream",
        ))
        .mount(&server)
        .await;

    let client = LlmClient::new(&cfg_for(&server, "m", 3)).expect("client");
    let err = client
        .complete_json("sys", "user")
        .await
        .expect_err("unparseable");

    assert!(
        matches!(err, LlmError::Unparseable(_)),
        "prose with no JSON must be Unparseable, got {err:?}"
    );
}

/// Criterion 24: an empty 200 body yields `Unparseable`.
///
/// "Empty body" here means an SSE response whose payload is empty - not an
/// HTTP 200 with no chunks at all. The SDK's stream completes successfully
/// and emits no `ContentBlock::Text`, which the parser reports as no JSON.
/// The contract: never an empty success.
#[tokio::test]
async fn empty_body_yields_unparseable() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(sse(&[""]), "text/event-stream"))
        .mount(&server)
        .await;

    let client = LlmClient::new(&cfg_for(&server, "m", 3)).expect("client");
    let err = client
        .complete_json("sys", "user")
        .await
        .expect_err("unparseable");

    assert!(
        matches!(err, LlmError::Unparseable(_)),
        "empty body must be Unparseable, got {err:?}"
    );
}

/// Criterion 25: a persistent 500 yields `Transport` after retrying.
///
/// The mock returns 500 for every request; we assert the mock was hit more
/// than once, proving the SDK's retry loop fired.
#[tokio::test]
async fn persistent_500_yields_transport_after_retrying() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(500).set_body_string("boom"))
        .mount(&server)
        .await;

    let mut cfg = cfg_for(&server, "m", 3);
    cfg.timeout_secs = 30;
    let client = fast_retry_client(&cfg);

    let err = client
        .complete_json("sys", "user")
        .await
        .expect_err("500 must error");
    assert!(
        matches!(err, LlmError::Transport(_)),
        "persistent 500 must be Transport, got {err:?}"
    );

    let calls = request_count(&server).await;
    assert!(
        calls > 1,
        "the SDK must retry on 500; mock was called {calls} time(s)"
    );
}

/// Criterion 26: a 400 yields an error WITHOUT retrying.
///
/// 400 is not in the SDK's retryable class (5xx only), so the SDK returns
/// immediately. We assert the mock was called exactly once.
#[tokio::test]
async fn error_400_is_not_retried() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(400).set_body_string("bad request"))
        .mount(&server)
        .await;

    let mut cfg = cfg_for(&server, "m", 3);
    cfg.timeout_secs = 30;
    let client = fast_retry_client(&cfg);

    let err = client
        .complete_json("sys", "user")
        .await
        .expect_err("400 must error");
    assert!(
        matches!(err, LlmError::Transport(_)),
        "non-retryable error must still surface as Transport, got {err:?}"
    );

    let calls = request_count(&server).await;
    assert_eq!(
        calls, 1,
        "400 must not retry; mock was called {calls} time(s)"
    );
}

/// Criterion 27: a 500 followed by a 200 succeeds. A transient failure
/// recovers without the caller having to do anything.
#[tokio::test]
async fn transient_500_followed_by_200_succeeds() {
    let server = MockServer::start().await;

    // First call: 500. `up_to_n_times(1)` makes this mock match at most
    // once; subsequent calls fall through to the success mock.
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(500).set_body_string("transient"))
        .up_to_n_times(1)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(sse(&["{\"findings\":[]}"]), "text/event-stream"),
        )
        .mount(&server)
        .await;

    let mut cfg = cfg_for(&server, "m", 3);
    cfg.timeout_secs = 30;
    let client = fast_retry_client(&cfg);

    let extracted = client
        .complete_json("sys", "user")
        .await
        .expect("transient 500 should recover");
    assert!(
        matches!(extracted, Extracted::Complete(_)),
        "recovered response must be Complete, got {extracted:?}"
    );

    let calls = request_count(&server).await;
    assert!(
        calls >= 2,
        "recovery requires the retry to actually fire; mock was called {calls} time(s)"
    );
}

/// Criterion 28: `max_retries: 0` in config still performs exactly one
/// attempt. The floor at 1 prevents the "zero attempts loop" failure mode
/// the spec calls out.
#[tokio::test]
async fn zero_max_retries_still_performs_one_attempt() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(500).set_body_string("boom"))
        .mount(&server)
        .await;

    let mut cfg = cfg_for(&server, "m", 0);
    cfg.timeout_secs = 30;
    let client = fast_retry_client(&cfg);

    let _ = client
        .complete_json("sys", "user")
        .await
        .expect_err("500 must error");
    assert_eq!(
        request_count(&server).await,
        1,
        "max_retries=0 must floor at 1 attempt, not skip the request entirely"
    );
}

/// Criterion 29: `Unparseable` is never retried.
///
/// The mock returns 200 with garbage. The SDK reports success (no
/// transport error), the parser returns None, and the retry layer sees
/// `Ok(None)` - so it stops. The mock must be called exactly once.
#[tokio::test]
async fn unparseable_is_never_retried() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(sse(&["this is not JSON, sorry"]), "text/event-stream"),
        )
        .mount(&server)
        .await;

    let mut cfg = cfg_for(&server, "m", 3);
    cfg.timeout_secs = 30;
    let client = fast_retry_client(&cfg);

    let err = client
        .complete_json("sys", "user")
        .await
        .expect_err("garbage body must error");
    assert!(
        matches!(err, LlmError::Unparseable(_)),
        "garbage body must be Unparseable, got {err:?}"
    );

    assert_eq!(
        request_count(&server).await,
        1,
        "Unparseable must not retry; the same prompt truncates the same way"
    );
}

/// Pins the SDK's retryable classification the test above depends on. If
/// this changes upstream, criterion 26's "400 not retried" assertion
/// silently becomes wrong - this test makes that drift visible.
#[test]
fn sdk_classifies_400_as_non_retryable() {
    let e400 = open_agent::Error::api("API error 400 Bad Request".to_string());
    assert!(
        !is_retryable_error(&e400),
        "the SDK must keep 400 in the non-retryable class"
    );
}
