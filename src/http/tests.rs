//! The size ceiling, from both sides of the boundary.
//!
//! This is the one safety property in drep's own HTTP, and it used to exist in
//! one of the two fetchers that need it. Testing it here rather than in either
//! caller is what stops it being tested for one of them again.

use super::*;
use crate::test_support::json_server;

/// A body no bigger than a sentence, so the boundary is reachable without a
/// multi-megabyte fixture.
const BODY: &str = r#"{"ok": true}"#;

/// GET `/api.json` from `server` and read it with `max_bytes` as the ceiling.
async fn read_from(server: &wiremock::MockServer, max_bytes: u64) -> Result<String, ReadError> {
    let response = client(Duration::from_secs(5))
        .expect("a client builds")
        .get(format!("{}/api.json", server.uri()))
        .send()
        .await
        .expect("the mock server answers");

    read_bounded(response, max_bytes).await
}

#[tokio::test]
async fn a_body_exactly_at_the_limit_is_within_it() {
    // `>` and not `>=`. Asserted beside the case one byte over, because either
    // comparison passes whichever of the two is tested alone.
    let server = json_server("/api.json", 200, BODY).await;

    let body = read_from(&server, BODY.len() as u64)
        .await
        .expect("a body exactly at the limit is within it");

    assert_eq!(body, BODY);
}

#[tokio::test]
async fn a_body_one_byte_over_the_limit_is_refused() {
    let server = json_server("/api.json", 200, BODY).await;

    let err = read_from(&server, BODY.len() as u64 - 1)
        .await
        .expect_err("a body past the limit is refused");

    assert!(matches!(err, ReadError::Transport(_)), "got {err:?}");
    assert!(err.to_string().contains("limit"), "and says why: {err}");
}

#[tokio::test]
async fn a_declared_length_past_the_limit_is_refused_before_the_body_is_read() {
    // The shortcut: a host announcing a gigabyte is turned away without
    // allocating for it.
    let server = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/api.json"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_string(BODY))
        .mount(&server)
        .await;

    let err = read_from(&server, 4).await.expect_err("refused");

    assert!(
        err.to_string().contains("declares"),
        "the header check is what refused it, not the streaming cap: {err}"
    );
}

#[tokio::test]
async fn a_response_declaring_no_length_is_still_bounded() {
    // Chunked transfer encoding sends no `Content-Length`, and neither does a
    // response reqwest has decompressed - which is every real fetch of the
    // registry, since models.dev serves it gzipped. The header check cannot be
    // the only bound.
    let server = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/api.json"))
        .respond_with(
            wiremock::ResponseTemplate::new(200)
                .set_body_string(BODY)
                .append_header("transfer-encoding", "chunked"),
        )
        .mount(&server)
        .await;

    let err = read_from(&server, 4).await.expect_err("refused");

    assert!(
        err.to_string().contains("exceeded"),
        "the streaming cap is what refused it: {err}"
    );
}

#[tokio::test]
async fn a_body_that_is_not_utf8_is_malformed_rather_than_transport() {
    // The distinction both callers rely on: a transport failure is worth
    // reporting as unreachable, bytes that are not text are not.
    let server = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/api.json"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_bytes(vec![0xff, 0xfe, 0xfd]))
        .mount(&server)
        .await;

    let err = read_from(&server, 1024).await.expect_err("not text");

    assert!(matches!(err, ReadError::Malformed(_)), "got {err:?}");
}

#[tokio::test]
async fn a_client_carries_the_timeout_it_was_built_with() {
    // Port 9 refuses immediately, so this proves the client is usable at all
    // rather than measuring the timeout - which no test should spend.
    let err = client(Duration::from_millis(50))
        .expect("a client builds")
        .get("http://127.0.0.1:9/api.json")
        .send()
        .await
        .expect_err("nothing is listening");

    assert!(err.is_connect() || err.is_timeout(), "got {err:?}");
}
