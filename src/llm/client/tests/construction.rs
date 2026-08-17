//! `LlmClient::new` - configuration validation.
//!
//! Criteria 20 and 21: an incomplete or disabled config must surface as
//! `LlmError::NotConfigured` at construction, not at the first request. A
//! gate that builds successfully and only fails on the first analyze call
//! would silently skip analysis of every file before the user noticed.

use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::support::{cfg_for, sse};
use crate::config::LlmConfig;
use crate::llm::client::{LlmClient, LlmError};

/// Criterion 20: `new` returns `NotConfigured` when `enabled` is false.
#[test]
fn new_returns_not_configured_when_disabled() {
    let cfg = LlmConfig {
        enabled: false,
        endpoint: Some("http://host/v1".into()),
        model: Some("m".into()),
        ..LlmConfig::default()
    };
    let err = LlmClient::new(&cfg).unwrap_err();
    assert!(
        matches!(err, LlmError::NotConfigured(_)),
        "expected NotConfigured, got {err:?}"
    );
}

/// Criterion 21a: `new` returns `NotConfigured` when `endpoint` is None.
#[test]
fn new_returns_not_configured_when_endpoint_missing() {
    let cfg = LlmConfig {
        enabled: true,
        endpoint: None,
        model: Some("m".into()),
        ..LlmConfig::default()
    };
    let err = LlmClient::new(&cfg).unwrap_err();
    assert!(
        matches!(err, LlmError::NotConfigured(_)),
        "expected NotConfigured, got {err:?}"
    );
}

/// Criterion 21b: `new` returns `NotConfigured` when `model` is None.
#[test]
fn new_returns_not_configured_when_model_missing() {
    let cfg = LlmConfig {
        enabled: true,
        endpoint: Some("http://host/v1".into()),
        model: None,
        ..LlmConfig::default()
    };
    let err = LlmClient::new(&cfg).unwrap_err();
    assert!(
        matches!(err, LlmError::NotConfigured(_)),
        "expected NotConfigured, got {err:?}"
    );
}

/// An unset cap must reach the wire as *no* `max_tokens` at all.
///
/// open-agent-sdk 0.7.0 omits the field when the setter is never called;
/// before that it substituted 4096, and drep compensated with a large
/// sentinel. This pins that the compensation is gone and not silently
/// reintroduced.
#[tokio::test]
async fn unset_max_tokens_is_absent_from_the_request() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(sse(&["{}"]), "text/event-stream"))
        .mount(&server)
        .await;

    let cfg = cfg_for(&server, "m", 3);
    assert_eq!(cfg.max_tokens, None, "fixture must leave the cap unset");
    let client = LlmClient::new(&cfg).expect("client");
    let _ = client.complete_json("sys", "user").await;

    let reqs = server.received_requests().await.expect("log");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body is JSON");
    assert!(
        body.get("max_tokens")
            .is_none_or(serde_json::Value::is_null),
        "an unset cap must not appear on the wire, got {:?}",
        body.get("max_tokens")
    );
}

/// A configured cap must actually be forwarded.
///
/// `LlmConfig::max_tokens` was parsed and tested but never read by the client -
/// it had no such field, so a user-set ceiling was silently discarded.
#[tokio::test]
async fn a_configured_max_tokens_is_forwarded() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(sse(&["{}"]), "text/event-stream"))
        .mount(&server)
        .await;

    let mut cfg = cfg_for(&server, "m", 3);
    cfg.max_tokens = Some(1234);
    let client = LlmClient::new(&cfg).expect("client");
    let _ = client.complete_json("sys", "user").await;

    let reqs = server.received_requests().await.expect("log");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body is JSON");
    assert_eq!(
        body["max_tokens"].as_u64(),
        Some(1234),
        "a configured cap must reach the model"
    );
}
