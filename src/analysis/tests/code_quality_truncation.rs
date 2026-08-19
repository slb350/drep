//! Truncation and caching: criteria 16, 17, 18.
//!
//! The two are entangled because the spec's rule is "never cache a
//! truncated response", and that rule is what makes a `Truncated` *
//! observable* — without it, a future caller could decide to cache it
//! and make one truncation permanent for the whole TTL. Each test
//! asserts on the request count where the rule has a network effect.

use std::path::PathBuf;

use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::support::{analyzer_for, hunks_for_python_at, hunks_for_python_at_two_lines};
use crate::test_support::{mount_sse, request_count, sse};

/// Criterion 16: a truncated response yields its partial findings AND marks
/// the file failed. Both assertions: one alone would let the wrong
/// implementation pass.
#[tokio::test]
async fn truncated_response_yields_partial_findings_and_marks_failed() {
    let server = MockServer::start().await;
    // The body ends with a terminated string but unclosed structure
    // (`}` and `]` missing). `extract_json` recovers the partial value as
    // `Truncated` by appending the missing closing delimiters.
    let body = "{\"issues\": [\
        {\"line\": 100, \"severity\": \"high\", \"category\": \"bug\", \"message\": \"first\"}, \
        {\"line\": 101, \"severity\": \"high\", \"category\": \"bug\", \"message\": \"sec\"";
    mount_sse(
        &server,
        ResponseTemplate::new(200).set_body_raw(sse(&[body]), "text/event-stream"),
    )
    .await;

    let (analyzer, _dir) = analyzer_for(&server);
    let result = analyzer
        .analyze_file(&hunks_for_python_at_two_lines())
        .await;

    assert!(
        !result.findings.is_empty(),
        "truncated response must yield at least one finding, got none"
    );
    assert!(
        result
            .failed_files
            .contains_key(&PathBuf::from("src/lib.py")),
        "truncated response must mark the file failed, got {:?}",
        result.failed_files
    );
}

/// Criterion 17: a truncated response is NOT cached. After a truncated
/// response, a second identical call issues a second HTTP request.
#[tokio::test]
async fn truncated_response_is_not_cached() {
    let server = MockServer::start().await;
    let body = "{\"issues\": [\
        {\"line\": 100, \"severity\": \"high\", \"category\": \"bug\", \"message\": \"x\"}, \
        {\"line\": 101, \"severity\": \"high\", \"category\": \"bug\", \"message\": \"y\"";
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(sse(&[body]), "text/event-stream"))
        .expect(2) // expected to be called twice
        .mount(&server)
        .await;

    let (analyzer, _dir) = analyzer_for(&server);
    let hunks = hunks_for_python_at_two_lines();

    let _ = analyzer.analyze_file(&hunks).await;
    let _ = analyzer.analyze_file(&hunks).await;

    assert_eq!(
        request_count(&server).await,
        2,
        "a truncated response must not be cached; the second call must hit the network"
    );
}

/// Criterion 18: a complete response IS cached. Two identical calls issue
/// exactly one HTTP request and return the same findings.
#[tokio::test]
async fn complete_response_is_cached() {
    let server = MockServer::start().await;
    let body = "{\"issues\": [\
        {\"line\": 100, \"severity\": \"high\", \"category\": \"bug\", \"message\": \"cached\"}\
    ], \"summary\": \"\"}";
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(sse(&[body]), "text/event-stream"))
        .expect(1)
        .mount(&server)
        .await;

    let (analyzer, _dir) = analyzer_for(&server);
    let hunks = hunks_for_python_at(100);

    let first = analyzer.analyze_file(&hunks).await;
    let second = analyzer.analyze_file(&hunks).await;

    assert_eq!(
        request_count(&server).await,
        1,
        "the second call must be served from the cache"
    );
    assert_eq!(first.findings, second.findings);
    assert_eq!(first.findings.len(), 1);
    assert_eq!(first.findings[0].message, "cached");
}
