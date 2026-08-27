//! Caller-supplied request headers, and the one drep sends without being asked.
//!
//! What the endpoint receives. The precedence between drep's default and an
//! operator's replacement is decided in `config::effective_headers` and pinned
//! there; these assert that whatever that resolves to is what goes out.

use std::collections::BTreeMap;

use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::llm::client::LlmClient;
use crate::test_support::{cfg_for, sse};

/// Mount one SSE reply that only matches when `name: value` is on the request.
///
/// The assertion is the matcher: an unmatched request gets no mock and the
/// client fails, so a header that did not arrive fails the test on the request
/// rather than on a value read back out of the config.
async fn server_requiring(name: &str, value: &str) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(header(name, value))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(sse(&["{\"findings\":[]}"]), "text/event-stream"),
        )
        .mount(&server)
        .await;
    server
}

#[tokio::test]
async fn a_configured_header_reaches_the_request() {
    let server = server_requiring("X-Floodgate-IsCode", "yes").await;
    let mut config = cfg_for(&server, "m", 1);
    config.headers = BTreeMap::from([("X-Floodgate-IsCode".to_owned(), "yes".to_owned())]);

    let client = LlmClient::new(&config).expect("client");

    client
        .complete_json("sys", "user")
        .await
        .expect("the configured header must be on the request the endpoint matched");
}

/// drep identifies itself even when nothing is configured.
///
/// A gateway that logs or bills per client cannot attribute a request carrying
/// no `User-Agent`, and reqwest sends none by default.
#[tokio::test]
async fn a_default_user_agent_names_drep_and_its_version() {
    let expected = format!("drep/{}", env!("CARGO_PKG_VERSION"));
    let server = server_requiring("User-Agent", &expected).await;

    let client = LlmClient::new(&cfg_for(&server, "m", 1)).expect("client");

    client
        .complete_json("sys", "user")
        .await
        .expect("an unconfigured run still names itself");
}

/// The discriminating half: the default is a default, not a fixed value.
#[tokio::test]
async fn a_configured_user_agent_replaces_the_default() {
    let server = server_requiring("User-Agent", "acme-gate/3.1").await;
    let mut config = cfg_for(&server, "m", 1);
    config.headers = BTreeMap::from([("User-Agent".to_owned(), "acme-gate/3.1".to_owned())]);

    let client = LlmClient::new(&config).expect("client");

    client
        .complete_json("sys", "user")
        .await
        .expect("a configured user agent wins over drep's own");
}

/// A caller header outranks the auth header the protocol would have sent.
///
/// This is the whole reason the ordering matters: an endpoint that wants a
/// bearer token the SDK's own scheme would not produce is reachable only if the
/// configured value replaces it rather than sitting beside it.
#[tokio::test]
async fn a_configured_authorization_replaces_the_protocol_default() {
    let server = server_requiring("Authorization", "Bearer caller-supplied").await;
    let mut config = cfg_for(&server, "m", 1);
    config.api_key = Some("from-the-config".to_owned());
    config.headers = BTreeMap::from([(
        "Authorization".to_owned(),
        "Bearer caller-supplied".to_owned(),
    )]);

    let client = LlmClient::new(&config).expect("client");

    client
        .complete_json("sys", "user")
        .await
        .expect("the configured Authorization is the one sent");
}

/// `{:?}` on a client prints header names and never their values.
///
/// The twin of the `LlmConfig` assertion. Both impls exist so that no `{:?}`,
/// `dbg!` or tracing line emits a credential, and this one held that rule by
/// comment alone.
#[test]
fn debug_prints_header_names_and_never_their_values() {
    let mut config = crate::config::LlmConfig {
        endpoint: Some("http://e/v1".to_owned()),
        model: Some("m".to_owned()),
        ..crate::config::LlmConfig::default()
    };
    config.headers =
        BTreeMap::from([("X-Tenant-Token".to_owned(), "super-secret-value".to_owned())]);

    let client = LlmClient::new(&config).expect("client");
    let rendered = format!("{client:?}");

    assert!(rendered.contains("X-Tenant-Token"), "got {rendered}");
    assert!(
        !rendered.contains("super-secret-value"),
        "a header value must never reach a log: {rendered}"
    );
}
