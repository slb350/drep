//! Caller-supplied request headers, and the one drep sends without being asked.
//!
//! What the endpoint receives. The precedence between drep's default and an
//! operator's replacement is decided in `config::effective_headers` and pinned
//! there; these assert that whatever that resolves to is what goes out.

use std::collections::BTreeMap;

use open_agent::ApiProtocol;
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
    server_requiring_at("/v1/chat/completions", name, value).await
}

async fn server_requiring_at(request_path: &str, name: &str, value: &str) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(request_path))
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

async fn assert_header_only_auth_omits_protocol_key(protocol: Option<&str>, protocol_header: &str) {
    let request_path = if protocol.is_some() {
        "/v1/messages"
    } else {
        "/v1/chat/completions"
    };
    let server = server_requiring_at(request_path, "X-Gateway-Key", "gateway-secret").await;
    let mut config = cfg_for(&server, "m", 1);
    config.api_key = None;
    config.protocol = protocol.map(str::to_owned);
    config.headers = BTreeMap::from([("X-Gateway-Key".to_owned(), "gateway-secret".to_owned())]);

    let client = LlmClient::new(&config).expect("client");

    // The shared fixture speaks OpenAI-flavoured SSE. Anthropic parsing may
    // reject the response body, but the request log below is the contract under
    // test and the mock only records it when the custom key arrived.
    let _ = client.complete_json("sys", "user").await;

    let requests = server.received_requests().await.expect("request log");
    assert_eq!(requests.len(), 1);
    assert!(
        !requests[0].headers.contains_key(protocol_header),
        "an absent api_key must not fabricate {protocol_header}: {:?}",
        requests[0].headers
    );
}

/// A gateway may authenticate outside the OpenAI protocol's default scheme.
/// An absent `api_key` therefore means no protocol Authorization header, not a
/// made-up bearer token that a strict gateway rejects before reading its own.
#[tokio::test]
async fn openai_header_only_auth_sends_no_protocol_key() {
    assert_header_only_auth_omits_protocol_key(None, "authorization").await;
}

/// The same contract over the Anthropic wire: a custom gateway key replaces
/// the need for `x-api-key` rather than travelling beside a fabricated value.
#[tokio::test]
async fn anthropic_header_only_auth_sends_no_protocol_key() {
    assert_header_only_auth_omits_protocol_key(Some(ApiProtocol::Anthropic.as_str()), "x-api-key")
        .await;
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

    let identity = client.request_identity();
    assert_eq!(identity.len(), 64, "request identity is a BLAKE3 digest");
    assert!(
        identity.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "request identity is hex: {identity}"
    );
    assert!(
        !identity.contains("super-secret-value"),
        "the long-lived identity must not retain header plaintext"
    );
}
