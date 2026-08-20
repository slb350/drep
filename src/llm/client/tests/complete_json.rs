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

use crate::llm::client::{Extracted, LlmClient};
use crate::llm::error::LlmError;
use crate::test_support::{
    cfg_for, fast_retry_client, mount_sse, request_count, server_finishing_with, server_returning,
    server_without_finish_reason, sse,
};

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

/// An empty response body is **retried**, and surfaces as `Transport`.
///
/// This test asserted the opposite until drep's own first gated push
/// disproved it: 7 of 49 files came back with no parseable JSON, and
/// re-running one immediately afterwards succeeded with findings. "The model
/// returned nothing" is provider flakiness, not a deterministic property of
/// the prompt - the same `finish_reason='error'` that blocked three
/// consecutive pushes under 1.x, where a single `max_retries` governed both
/// failure classes.
///
/// The request count is the load-bearing assertion. Without it, a
/// implementation that classified the empty body correctly but still refused
/// to retry would pass.
#[tokio::test]
async fn an_empty_body_is_retried_and_becomes_transport() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(sse(&[""]), "text/event-stream"))
        .mount(&server)
        .await;

    let cfg = cfg_for(&server, "m", 3);
    let err = fast_retry_client(&cfg)
        .complete_json("sys", "user")
        .await
        .expect_err("an endlessly empty response must fail the file");

    assert!(
        matches!(err, LlmError::Transport { .. }),
        "an empty body is a transport failure, not a parse failure, got {err:?}"
    );
    assert!(
        request_count(&server).await > 1,
        "and it must have been retried; the endpoint saw only one request"
    );
}

/// Whitespace-only counts as empty.
///
/// A provider that returns a lone newline has said nothing, and treating that
/// as "unparseable prose" would put it back on the non-retrying path.
#[tokio::test]
async fn a_whitespace_only_body_is_treated_as_empty() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(sse(&["\n  \n"]), "text/event-stream"),
        )
        .mount(&server)
        .await;

    let cfg = cfg_for(&server, "m", 3);
    let err = fast_retry_client(&cfg)
        .complete_json("sys", "user")
        .await
        .expect_err("whitespace is not an answer");

    assert!(matches!(err, LlmError::Transport { .. }), "got {err:?}");
    assert!(request_count(&server).await > 1, "must retry");
}

/// A **non-empty** unparseable body stays `Unparseable` - it never becomes
/// `Transport`, however many times it is asked for again.
///
/// This is the half of the split that still matters, and it is load-bearing
/// further up: `Transport` fails over to the next provider *and* demotes this
/// one for the rest of the run. A model that answered in prose has told us
/// nothing about the endpoint, so neither is warranted. The request count is
/// no longer the discriminator - both cases retry now - so the classification
/// is what has to be asserted.
#[tokio::test]
async fn a_non_empty_unparseable_body_stays_unparseable_rather_than_transport() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            sse(&["I am afraid I cannot help with that."]),
            "text/event-stream",
        ))
        .mount(&server)
        .await;

    let cfg = cfg_for(&server, "m", 3);
    let err = fast_retry_client(&cfg)
        .complete_json("sys", "user")
        .await
        .expect_err("prose is not JSON");

    assert!(
        matches!(err, LlmError::Unparseable(_)),
        "prose says nothing about the endpoint, so it must not be classified \
         as a transport failure, got {err:?}"
    );
    assert_eq!(
        request_count(&server).await,
        crate::llm::client::NO_JSON_ATTEMPTS as usize,
        "asked again, but a bounded number of times"
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
        matches!(err, LlmError::Transport { .. }),
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
        matches!(err, LlmError::Transport { .. }),
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

/// Pins the SDK's retryable classification the test above depends on. If
/// this changes upstream, criterion 26's "400 not retried" assertion
/// silently becomes wrong - this test makes that drift visible.
#[test]
fn sdk_classifies_400_as_non_retryable() {
    // api_status, not api: the latter leaves `status: None`, so the assertion
    // would pass merely because status-less errors are unretryable - it could
    // not tell that apart from "400 is classified non-retryable".
    let e400 = open_agent::Error::api_status(400, "Bad Request");
    assert!(
        !is_retryable_error(&e400),
        "the SDK must keep 400 in the non-retryable class"
    );
}

// ---- no JSON at all: retried, and reported with the body ----

/// A response with no JSON is asked for again, and a second answer that
/// parses is accepted.
///
/// The rule this replaced never retried, justified as "the same prompt
/// truncates the same way" - but that is `Extracted::Truncated`, a different
/// branch. A response with no JSON at all did not truncate an answer, it never
/// produced one, and drep's own gated push failed on a *different* file each
/// run with every failing file analyzing cleanly when asked again.
#[tokio::test]
async fn a_response_with_no_json_is_retried_and_a_later_answer_is_accepted() {
    let server = MockServer::start().await;
    // First call: prose, no JSON anywhere in it.
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            sse(&["Let me take a look at this file..."]),
            "text/event-stream",
        ))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    // Second call: a clean answer.
    mount_sse(
        &server,
        ResponseTemplate::new(200).set_body_raw(sse(&[r#"{"issues": []}"#]), "text/event-stream"),
    )
    .await;

    let client = fast_retry_client(&cfg_for(&server, "m", 1));
    let extracted = client
        .complete_json("sys", "content")
        .await
        .expect("the second attempt parses");

    assert!(matches!(extracted, Extracted::Complete(_)));
    assert_eq!(
        request_count(&server).await,
        2,
        "the prose answer must have been asked again"
    );
}

/// Retries are bounded, and the failure names what the model actually said.
///
/// The body used to be discarded behind the constant "response contained no
/// parseable JSON", so every occurrence looked identical and there was no way
/// to tell a refusal from a prose preamble from reasoning that leaked into the
/// content channel.
#[tokio::test]
async fn a_persistently_unparseable_response_is_bounded_and_reports_the_body() {
    let server = server_returning(&["I am afraid I cannot help with that."]).await;
    let client = fast_retry_client(&cfg_for(&server, "m", 1));

    let err = client
        .complete_json("sys", "content")
        .await
        .expect_err("prose never parses");

    match err {
        LlmError::Unparseable(message) => {
            assert!(
                message.contains("I am afraid I cannot help"),
                "the message must carry what came back, got {message:?}"
            );
        }
        other => panic!("expected Unparseable, got {other:?}"),
    }
    assert_eq!(
        request_count(&server).await,
        crate::llm::client::NO_JSON_ATTEMPTS as usize,
        "bounded: each attempt is a full reasoning call"
    );
}

/// A truncated response is NOT retried.
///
/// The discriminating counterpart. Truncation is the genuinely deterministic
/// case - the same prompt is cut at the same place - and it already yields a
/// usable partial value, so asking again buys a second full-price call for the
/// same answer. A rule that retried "anything that did not parse cleanly"
/// would pass the two tests above and be wrong here.
#[tokio::test]
async fn a_truncated_response_is_not_retried() {
    let server = server_returning(&[r#"{"issues": [{"line": 1,"#]).await;
    let client = fast_retry_client(&cfg_for(&server, "m", 1));

    let extracted = client
        .complete_json("sys", "content")
        .await
        .expect("brace-balancing recovers a prefix");

    assert!(
        matches!(extracted, Extracted::Truncated(_)),
        "got {extracted:?}"
    );
    assert_eq!(
        request_count(&server).await,
        1,
        "a truncated answer is deterministic - asking again buys nothing"
    );
}

/// A control character in the model's answer cannot reach the terminal.
///
/// The excerpt lands in a terminal report and in `--format json`. The text is
/// model output, so an escape sequence in it would otherwise be interpreted by
/// whatever is reading the report.
#[tokio::test]
async fn the_reported_body_is_stripped_of_control_characters() {
    let server = server_returning(&["prose \u{1b}[31mred\u{1b}[0m and\nnewlines"]).await;
    let client = fast_retry_client(&cfg_for(&server, "m", 1));

    let err = client
        .complete_json("sys", "content")
        .await
        .expect_err("prose never parses");
    let message = err.to_string();

    assert!(
        !message.contains('\u{1b}'),
        "an escape sequence must not survive into the report: {message:?}"
    );
    assert!(
        !message.contains('\n'),
        "the excerpt is one line: {message:?}"
    );
    assert!(
        message.contains("red"),
        "the text itself survives: {message}"
    );
}

// ---- the server said why it stopped ----

/// A response cut off at the output token cap is NOT retried.
///
/// This is the genuinely deterministic case: drep sends no `max_tokens`, so the
/// cap is the server's, and the same request hits the same cap every time.
/// Retrying is pure spend. The original "never retry a non-empty body" rule was
/// reaching for exactly this, but identified it by "no JSON in the body" - the
/// wrong proxy, because a model that merely chose prose *will* answer
/// differently next time.
#[tokio::test]
async fn a_response_cut_off_at_the_token_cap_is_not_retried() {
    let server = server_finishing_with(&["Let me start by reading the file"], "length").await;
    let client = fast_retry_client(&cfg_for(&server, "m", 1));

    let err = client
        .complete_json("sys", "content")
        .await
        .expect_err("no JSON was produced");

    match err {
        LlmError::ModelStopped { finish, message } => {
            assert_eq!(finish, "length", "the server's own word, kept as a tag");
            assert!(
                message.contains("output token limit"),
                "the message must name the cause, got {message:?}"
            );
            assert!(
                message.contains("split it"),
                "and be actionable, got {message:?}"
            );
        }
        other => panic!("expected ModelStopped, got {other:?}"),
    }
    assert_eq!(
        request_count(&server).await,
        1,
        "the same request hits the same cap - asking again is pure spend"
    );
}

/// A content filter refusal is NOT retried either, and says so.
#[tokio::test]
async fn a_content_filter_refusal_is_not_retried() {
    let server = server_finishing_with(&["I cannot process this"], "content_filter").await;
    let client = fast_retry_client(&cfg_for(&server, "m", 1));

    let err = client
        .complete_json("sys", "content")
        .await
        .expect_err("no JSON was produced");

    match err {
        LlmError::ModelStopped { finish, message } => {
            assert_eq!(finish, "content_filter");
            assert!(
                !message.contains("output token limit"),
                "a filter refusal is not a budget problem, got {message:?}"
            );
        }
        other => panic!("expected ModelStopped, got {other:?}"),
    }
    assert_eq!(request_count(&server).await, 1);
}

/// A model that merely *stopped* without JSON IS retried.
///
/// The discriminating counterpart to both tests above. Same observable body -
/// prose, no JSON - and the opposite decision, which only the finish reason can
/// justify. A rule keyed on the body would treat all three identically, which is
/// precisely the bug this replaced.
#[tokio::test]
async fn a_model_that_stopped_without_json_is_still_retried() {
    let server = server_finishing_with(&["I am afraid I cannot help"], "stop").await;
    let client = fast_retry_client(&cfg_for(&server, "m", 1));

    let err = client
        .complete_json("sys", "content")
        .await
        .expect_err("prose never parses");

    assert!(
        matches!(err, LlmError::Unparseable(_)),
        "a model that chose prose may choose differently next time, got {err:?}"
    );
    assert_eq!(
        request_count(&server).await,
        crate::llm::client::NO_JSON_ATTEMPTS as usize
    );
}

/// A server that never reports a reason is retried.
///
/// `Unspecified` means "no information", which several OpenAI-compatible
/// servers give by default. It must not be read as `Stop` - but it must not
/// stop the retry either, because nothing rules a different answer out.
///
/// The fixture reports no reason on any chunk, which is what the name says
/// and what only open-agent-sdk 0.10.0 makes expressible: under 0.9.x such a
/// stream yielded no text at all, so the test had to settle for a `"stop"`
/// body and was pinning `Stop` while claiming to pin `Unspecified`.
#[tokio::test]
async fn a_response_with_no_finish_reason_is_retried() {
    let server = server_without_finish_reason(&["still not JSON"]).await;
    let client = fast_retry_client(&cfg_for(&server, "m", 1));

    let err = client
        .complete_json("sys", "content")
        .await
        .expect_err("prose never parses");
    assert!(matches!(err, LlmError::Unparseable(_)), "got {err:?}");
    assert_eq!(
        request_count(&server).await,
        crate::llm::client::NO_JSON_ATTEMPTS as usize
    );
}

/// A capped response that *did* produce parseable JSON is still accepted.
///
/// `Length` only ends the attempt when there is no JSON to show for it. A
/// response that closed its own braces before the cap is a complete answer,
/// whatever happened after it.
#[tokio::test]
async fn a_capped_response_that_still_produced_json_is_accepted() {
    let server = server_finishing_with(&[r#"{"issues": []}"#], "length").await;
    let client = fast_retry_client(&cfg_for(&server, "m", 1));

    let extracted = client
        .complete_json("sys", "content")
        .await
        .expect("the JSON parsed, so the cap is irrelevant");
    assert!(matches!(extracted, Extracted::Complete(_)));
    assert_eq!(request_count(&server).await, 1);
}
