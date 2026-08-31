//! Redirects from the authenticated model-listing request.
//!
//! This path does not go through open-agent-sdk: `drep init` owns its one
//! `GET /models`, including the protocol credential. These tests therefore pin
//! the same exact-origin rule independently for drep's client.

use super::*;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

async fn mount_redirect(server: &MockServer, location: &str) {
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(ResponseTemplate::new(307).insert_header("location", location))
        .mount(server)
        .await;
}

async fn mount_listing(server: &MockServer, route: &str, body: &str) {
    Mock::given(method("GET"))
        .and(path(route))
        .respond_with(ResponseTemplate::new(200).set_body_string(body.to_owned()))
        .mount(server)
        .await;
}

async fn requests(server: &MockServer) -> Vec<Request> {
    server.received_requests().await.expect("records requests")
}

fn header<'a>(request: &'a Request, name: &str) -> &'a str {
    request
        .headers
        .get(name)
        .expect("request carries the credential")
        .to_str()
        .expect("credential header is text")
}

fn assert_origin_redirect(result: Result<Vec<Model>, ListError>) {
    match result {
        Err(ListError::Transport(message)) => {
            assert!(message.contains("307"), "origin status was lost: {message}");
        }
        other => panic!("the origin redirect must be surfaced, got {other:?}"),
    }
}

#[tokio::test]
async fn an_anthropic_listing_never_sends_x_api_key_to_a_cross_origin_redirect() {
    let target = MockServer::start().await;
    mount_listing(&target, "/redirected", MINIMAX).await;
    let origin = MockServer::start().await;
    mount_redirect(&origin, &format!("{}/redirected", target.uri())).await;

    let result = Http::new()
        .list(
            &format!("{}/v1", origin.uri()),
            "anthropic-secret",
            ApiProtocol::Anthropic,
        )
        .await;
    let origin_requests = requests(&origin).await;
    let target_requests = requests(&target).await;

    assert_eq!(origin_requests.len(), 1);
    assert_eq!(header(&origin_requests[0], "x-api-key"), "anthropic-secret");
    assert!(
        target_requests.is_empty(),
        "redirect target received the authenticated request: {target_requests:?}"
    );
    assert_origin_redirect(result);
}

#[tokio::test]
async fn an_openai_listing_never_replays_authorization_on_a_same_origin_redirect() {
    let server = MockServer::start().await;
    mount_listing(&server, "/redirected", ZAI).await;
    mount_redirect(&server, &format!("{}/redirected", server.uri())).await;

    let result = Http::new()
        .list(
            &format!("{}/v1", server.uri()),
            "openai-secret",
            ApiProtocol::OpenAiChat,
        )
        .await;
    let requests = requests(&server).await;

    assert_eq!(
        requests.len(),
        1,
        "same-origin redirect received the authenticated request: {requests:?}"
    );
    assert_eq!(requests[0].url.path(), "/v1/models");
    assert_eq!(
        header(&requests[0], "authorization"),
        "Bearer openai-secret"
    );
    assert_origin_redirect(result);
}
