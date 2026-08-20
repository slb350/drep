//! The HTTP path itself, against a mock server.
//!
//! Everything else here tests distillation, resolution and caching in
//! isolation. What only these can establish is that [`Http`] wires them
//! together: a 200 read as a document rather than as an error, and a non-2xx
//! read as a failure rather than parsed as one.

use super::super::{Fetch, Http, QuirksError, Registry};
use super::DOCUMENT;
use crate::llm::quirks::MAX_DOCUMENT_BYTES;

use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// A server answering `GET /api.json` with `body` and `status`.
async fn server(status: u16, body: &str) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api.json"))
        .respond_with(
            ResponseTemplate::new(status)
                .insert_header("content-type", "application/json")
                .set_body_string(body.to_string()),
        )
        .mount(&server)
        .await;
    server
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
async fn a_body_at_the_limit_is_accepted_and_one_byte_over_is_not() {
    // The boundary, asserted from both sides. A production-sized check would
    // need a 32 MB body, which is why the ceiling is a field.
    let document = r#"{"p": {"api": "https://e/v1", "models": {}}}"#;
    let size = document.len() as u64;

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api.json"))
        .respond_with(ResponseTemplate::new(200).set_body_string(document.to_string()))
        .mount(&server)
        .await;
    let url = format!("{}/api.json", server.uri());

    // Exactly at the limit: allowed. `>` and not `>=`.
    Http::new(&url)
        .with_max_bytes(size)
        .document()
        .await
        .expect("a body exactly at the limit is within it");

    // One byte under the body's size: refused.
    let err = Http::new(&url)
        .with_max_bytes(size - 1)
        .document()
        .await
        .expect_err("a body past the limit is refused");
    assert!(matches!(err, QuirksError::Transport(_)), "got {err:?}");
}

#[tokio::test]
async fn a_response_declaring_no_length_is_still_bounded() {
    // Chunked transfer encoding sends no `Content-Length`, so the header check
    // cannot be the only bound - the streaming read has to enforce it too.
    let document = r#"{"p": {"api": "https://e/v1", "models": {}}}"#;
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api.json"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(document.to_string())
                .append_header("transfer-encoding", "chunked"),
        )
        .mount(&server)
        .await;

    let err = Http::new(&format!("{}/api.json", server.uri()))
        .with_max_bytes(4)
        .document()
        .await
        .expect_err("the streaming read enforces the limit too");

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
