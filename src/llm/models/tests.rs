//! Unit tests for model listing.
//!
//! The response bodies here are trimmed copies of what the three subscription
//! endpoints actually returned on 2026-08-19, field sets intact. A synthetic
//! body would not reproduce the thing that matters: the three vendors agree on
//! `data[].id` and on nothing else.

use super::*;

/// z.ai, OpenAI-shaped: `object`/`created`/`owned_by`, no display name.
const ZAI: &str = r#"{"object":"list","data":[
    {"id":"glm-5.3","object":"model","created":1766332800,"owned_by":"z-ai"},
    {"id":"glm-5.2","object":"model","created":1766332800,"owned_by":"z-ai"},
    {"id":"glm-4.7","object":"model","created":1766332800,"owned_by":"z-ai"}
]}"#;

/// MiniMax, Anthropic-shaped: `type`/`created_at`/`display_name`.
const MINIMAX: &str = r#"{"data":[
    {"id":"MiniMax-M3","type":"model","display_name":"MiniMax-M3",
     "created_at":"2026-06-01T00:00:00Z"},
    {"id":"MiniMax-M2.7-highspeed","type":"model","display_name":"MiniMax-M2.7-Highspeed",
     "created_at":"2026-03-18T02:00:00Z"}
]}"#;

/// Kimi, both shapes plus capability metadata.
const KIMI: &str = r#"{"data":[
    {"id":"kimi-for-coding","created":1761264000,"created_at":"2025-10-24T00:00:00Z",
     "object":"model","display_name":"K2.7 Coding","type":"model","context_length":262144,
     "supports_reasoning":true,"supports_image_in":true,"supports_thinking_type":"only"},
    {"id":"k3","created":1761264000,"object":"model","display_name":"K3","type":"model",
     "context_length":1048576,"supports_reasoning":true}
]}"#;

fn ids(body: &str) -> Vec<String> {
    parse(body)
        .expect("parses")
        .into_iter()
        .map(|model| model.id)
        .collect()
}

#[test]
fn an_openai_shaped_listing_parses() {
    assert_eq!(ids(ZAI), vec!["glm-5.3", "glm-5.2", "glm-4.7"]);
}

#[test]
fn an_anthropic_shaped_listing_parses() {
    assert_eq!(ids(MINIMAX), vec!["MiniMax-M3", "MiniMax-M2.7-highspeed"]);
}

#[test]
fn a_listing_carrying_capability_metadata_parses() {
    // The fields drep does not read must not break the parse, or a vendor
    // adding one takes the feature out.
    assert_eq!(ids(KIMI), vec!["kimi-for-coding", "k3"]);
}

#[test]
fn a_display_name_is_kept_when_the_endpoint_sends_one() {
    let models = parse(KIMI).expect("parses");

    assert_eq!(models[0].display_name.as_deref(), Some("K2.7 Coding"));
}

#[test]
fn a_listing_without_display_names_still_parses() {
    let models = parse(ZAI).expect("parses");

    assert!(models[0].display_name.is_none());
}

#[test]
fn the_endpoints_own_order_is_preserved() {
    // Every one of these lists its newest model first, which is what a user
    // setting drep up almost always wants. Sorting would bury it: `MiniMax-M2`
    // sorts above `MiniMax-M3`, and `glm-4.7` above `glm-5.3`.
    let models = parse(ZAI).expect("parses");

    assert_eq!(models[0].id, "glm-5.3", "newest first, not alphabetical");
}

#[test]
fn a_label_shows_the_vendors_name_beside_the_id() {
    // `k3` is displayed as "K2.7 Coding" by its own vendor, which nobody would
    // guess from the id they have to put in the config.
    let model = Model {
        id: "kimi-for-coding".to_string(),
        display_name: Some("K2.7 Coding".to_string()),
    };

    assert_eq!(model.label(), "kimi-for-coding (K2.7 Coding)");
}

#[test]
fn a_label_does_not_repeat_a_display_name_equal_to_the_id() {
    // MiniMax sends `display_name` equal to `id` for every model. Rendering
    // "MiniMax-M3 (MiniMax-M3)" would be noise on the most common listing.
    let model = Model {
        id: "MiniMax-M3".to_string(),
        display_name: Some("MiniMax-M3".to_string()),
    };

    assert_eq!(model.label(), "MiniMax-M3");
}

#[test]
fn a_label_without_a_display_name_is_the_id() {
    let model = Model {
        id: "glm-5.3".to_string(),
        display_name: None,
    };

    assert_eq!(model.label(), "glm-5.3");
}

#[test]
fn an_empty_listing_is_unsupported_rather_than_an_empty_menu() {
    // A prompt offering nothing to pick is worse than the free-text prompt it
    // replaced.
    let err = parse(r#"{"data":[]}"#).expect_err("an empty list is not a listing");

    assert!(matches!(err, ListError::Unsupported), "got {err:?}");
}

#[test]
fn an_entry_with_no_id_is_dropped() {
    let models = parse(r#"{"data":[{"id":""},{"id":"real"}]}"#).expect("parses");

    assert_eq!(models.len(), 1);
    assert_eq!(models[0].id, "real");
}

#[test]
fn a_body_that_is_not_a_listing_is_malformed() {
    let err = parse("not json at all").expect_err("unparseable");

    assert!(matches!(err, ListError::Malformed(_)), "got {err:?}");
}

#[test]
fn a_json_body_with_no_data_array_is_malformed() {
    let err = parse(r#"{"models":["a"]}"#).expect_err("wrong shape");

    assert!(matches!(err, ListError::Malformed(_)), "got {err:?}");
}

#[test]
fn a_malformed_message_is_bounded_and_stripped() {
    // The same rule as every other piece of text drep did not write: it lands
    // in a terminal, so control characters cannot survive.
    let err = parse("\u{1b}[31mnot json\u{1b}[0m").expect_err("unparseable");

    let message = err.to_string();
    assert!(!message.contains('\u{1b}'), "escape survived: {message:?}");
}

#[test]
fn a_missing_route_is_unsupported_rather_than_a_failure() {
    // The ordinary case for a local server, and the reason nothing here is
    // fatal.
    for status in [404, 405, 501] {
        assert!(
            matches!(classify(status), ListError::Unsupported),
            "status {status}"
        );
    }
}

#[test]
fn a_rejected_key_is_reported_as_such() {
    // Worth separating: the user is about to store this key, and finding out it
    // is wrong now beats finding out on the first push.
    for status in [401, 403] {
        match classify(status) {
            ListError::Unauthorized(code) => assert_eq!(code, status),
            other => panic!("status {status} gave {other:?}"),
        }
    }
}

#[test]
fn any_other_status_is_a_transport_failure_naming_it() {
    match classify(500) {
        ListError::Transport(message) => assert!(message.contains("500"), "got {message}"),
        other => panic!("expected Transport, got {other:?}"),
    }
}

#[test]
fn the_listing_url_is_models_under_the_configured_base() {
    assert_eq!(
        url("https://api.z.ai/api/coding/paas/v4"),
        "https://api.z.ai/api/coding/paas/v4/models"
    );
    assert_eq!(
        url("https://api.minimax.io/anthropic/v1"),
        "https://api.minimax.io/anthropic/v1/models"
    );
}

#[test]
fn a_trailing_slash_does_not_produce_a_double_slash() {
    // Some gateways answer `//models` with a redirect and others with a 404,
    // which would report a working endpoint as having no listing.
    assert_eq!(
        url("http://localhost:1234/v1/"),
        "http://localhost:1234/v1/models"
    );
}

#[test]
fn every_error_reads_as_something_other_than_a_crash() {
    // These are shown to a user mid-setup, right before they type a name
    // instead. Each has to say which of the four things happened.
    let messages = [
        ListError::Unsupported.to_string(),
        ListError::Unauthorized(401).to_string(),
        ListError::Transport("timed out".to_string()).to_string(),
        ListError::Malformed("bad".to_string()).to_string(),
    ];

    let mut unique = messages.to_vec();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), 4, "messages collide: {messages:?}");
}

/// Tests for the HTTP path itself, against a mock server.
///
/// Everything above tests the parsing and classification in isolation. What
/// only these can establish is that `Http::list` wires them together: the right
/// URL, the right auth header for the protocol, and a *successful* response
/// read as a listing rather than as an error.
mod http {
    use super::*;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// A server answering `GET /v1/models` with `body` and `status`.
    async fn server(status: u16, body: &str) -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
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
    async fn a_successful_listing_is_read_as_models() {
        // The success check itself: reading a 200 as an error would mean the
        // menu never appears for any endpoint that serves one.
        let server = server(200, ZAI).await;

        let models = Http
            .list(
                &format!("{}/v1", server.uri()),
                "k",
                ApiProtocol::OpenAiChat,
            )
            .await
            .expect("a 200 listing parses");

        assert_eq!(models[0].id, "glm-5.3");
        assert_eq!(models.len(), 3);
    }

    #[tokio::test]
    async fn an_openai_endpoint_is_asked_with_a_bearer_token() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .and(header("authorization", "Bearer sk-test"))
            .respond_with(ResponseTemplate::new(200).set_body_string(ZAI.to_string()))
            .mount(&server)
            .await;

        // The mock only answers when the header matches, so parsing at all is
        // the assertion.
        Http.list(
            &format!("{}/v1", server.uri()),
            "sk-test",
            ApiProtocol::OpenAiChat,
        )
        .await
        .expect("the bearer header was sent");
    }

    #[tokio::test]
    async fn an_anthropic_endpoint_is_asked_with_x_api_key_and_a_version() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .and(header("x-api-key", "sk-test"))
            .and(header("anthropic-version", ANTHROPIC_VERSION))
            .respond_with(ResponseTemplate::new(200).set_body_string(MINIMAX.to_string()))
            .mount(&server)
            .await;

        let models = Http
            .list(
                &format!("{}/v1", server.uri()),
                "sk-test",
                ApiProtocol::Anthropic,
            )
            .await
            .expect("the anthropic headers were sent");

        assert_eq!(models[0].id, "MiniMax-M3");
    }

    #[tokio::test]
    async fn a_bearer_token_is_not_sent_to_an_anthropic_endpoint() {
        // The credential must not leak to a header the endpoint has no use for,
        // which is the same rule the completion path holds.
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_string(MINIMAX.to_string()))
            .mount(&server)
            .await;

        Http.list(
            &format!("{}/v1", server.uri()),
            "sk-test",
            ApiProtocol::Anthropic,
        )
        .await
        .expect("lists");

        let requests = server.received_requests().await.expect("records requests");
        assert!(
            requests[0].headers.get("authorization").is_none(),
            "a bearer token reached an Anthropic endpoint"
        );
    }

    #[tokio::test]
    async fn a_missing_route_is_reported_as_unsupported() {
        let server = server(404, "not found").await;

        let err = Http
            .list(
                &format!("{}/v1", server.uri()),
                "k",
                ApiProtocol::OpenAiChat,
            )
            .await
            .expect_err("404 is not a listing");

        assert!(matches!(err, ListError::Unsupported), "got {err:?}");
    }

    #[tokio::test]
    async fn a_rejected_key_is_reported_before_it_is_stored() {
        let server = server(401, "nope").await;

        let err = Http
            .list(
                &format!("{}/v1", server.uri()),
                "wrong",
                ApiProtocol::OpenAiChat,
            )
            .await
            .expect_err("401 is not a listing");

        assert!(matches!(err, ListError::Unauthorized(401)), "got {err:?}");
    }

    #[tokio::test]
    async fn a_success_with_an_unreadable_body_is_malformed_not_unsupported() {
        // The two mean different things to the caller: one is "this endpoint has
        // no listing", the other is "it has one and drep could not read it".
        let server = server(200, "<html>nope</html>").await;

        let err = Http
            .list(
                &format!("{}/v1", server.uri()),
                "k",
                ApiProtocol::OpenAiChat,
            )
            .await
            .expect_err("html is not a listing");

        assert!(matches!(err, ListError::Malformed(_)), "got {err:?}");
    }

    #[tokio::test]
    async fn an_unreachable_endpoint_is_a_transport_failure() {
        // Port 9 refuses immediately. This is the path a local server that is
        // simply not running takes.
        let err = Http
            .list("http://127.0.0.1:9/v1", "k", ApiProtocol::OpenAiChat)
            .await
            .expect_err("nothing is listening");

        assert!(matches!(err, ListError::Transport(_)), "got {err:?}");
    }
}
