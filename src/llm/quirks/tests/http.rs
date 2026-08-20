//! The HTTP path itself, against a mock server.
//!
//! Everything else here tests distillation, resolution and caching in
//! isolation. What only these can establish is that [`Http`] wires them
//! together: a 200 read as a document rather than as an error, and a non-2xx
//! read as a failure rather than parsed as one.

use super::super::{Fetch, Http, QuirksError, Registry};
use super::DOCUMENT;
use crate::llm::quirks::MAX_DOCUMENT_BYTES;
use crate::test_support::json_server;

use wiremock::MockServer;

/// A server answering `GET /api.json` with `body` and `status`.
async fn server(status: u16, body: &str) -> MockServer {
    json_server("/api.json", status, body).await
}

#[tokio::test]
async fn a_successful_response_is_the_document() {
    // The success check itself: reading a 200 as a failure would mean every
    // provider silently kept its preset's guess, which is indistinguishable
    // from working.
    let server = server(200, DOCUMENT).await;

    let body = Http::new(&format!("{}/api.json", server.uri()))
        .document()
        .await
        .expect("a 200 is the document");

    let registry = Registry::distil(&body, 0).expect("and it distils");
    assert!(
        registry
            .facts("https://api.kimi.com/coding/v1", "k3")
            .is_some()
    );
}

#[tokio::test]
async fn a_non_success_status_is_a_transport_failure() {
    // A CDN error page is valid JSON often enough that parsing it first would
    // produce a "registry" of nothing, cached for a week.
    let server = server(503, DOCUMENT).await;

    let err = Http::new(&format!("{}/api.json", server.uri()))
        .document()
        .await
        .expect_err("503 is not a document");

    assert!(matches!(err, QuirksError::Transport(_)), "got {err:?}");
    assert!(err.to_string().contains("503"), "and names it: {err}");
}

#[tokio::test]
async fn an_unreachable_host_is_a_transport_failure() {
    // Port 9 refuses immediately. This is the offline path, which every fetch
    // failure has to land on without stopping `drep init`.
    let err = Http::new("http://127.0.0.1:9/api.json")
        .document()
        .await
        .expect_err("nothing is listening");

    assert!(matches!(err, QuirksError::Transport(_)), "got {err:?}");
}

#[tokio::test]
async fn a_body_past_the_ceiling_is_a_transport_failure() {
    // The boundary itself belongs to `crate::http`, which owns the ceiling and
    // tests it from both sides. What is this module's to prove is the mapping:
    // an oversized body has to arrive as `QuirksError::Transport` and not as
    // `Malformed`, because the two are handled differently - one is a host drep
    // could not read from, the other a document it read and could not use.
    let server = server(200, DOCUMENT).await;

    let err = Http::new(&format!("{}/api.json", server.uri()))
        .with_max_bytes(4)
        .document()
        .await
        .expect_err("a body past the limit is refused");

    assert!(matches!(err, QuirksError::Transport(_)), "got {err:?}");
}

#[test]
fn the_production_ceiling_is_thirty_two_megabytes() {
    // Pins the arithmetic. The live document is about 4 MB, so this is a wide
    // margin for growth that still refuses a mirror serving something else.
    assert_eq!(MAX_DOCUMENT_BYTES, 33_554_432);
    assert_eq!(
        Http::new("https://e/api.json").max_bytes,
        MAX_DOCUMENT_BYTES
    );
}
