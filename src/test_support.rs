//! Shared test fixtures for everything that talks to a mock LLM endpoint.
//!
//! Crate-level rather than per-module because the LLM client tests and the
//! analysis tests need exactly the same four helpers. They were duplicated
//! once - byte-identical except that the copy dropped the doc paragraph
//! explaining why `fast_retry_client` must not override `max_attempts`, which
//! is the paragraph recording a real bug. Two copies of the SSE builder in
//! particular is a trap: it encodes an SDK behaviour that fails silently.
//!
//! The SSE builder here is the one piece that cannot be guessed: the SDK
//! buffers text deltas and only emits `ContentBlock`s when a chunk carries a
//! non-null `finish_reason`. A stream where every chunk has `"finish_reason":
//! null` yields ZERO blocks, with no error and no warning, because the empty
//! result is silently dropped.

use std::time::Duration;

use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::config::LlmConfig;
use crate::llm::client::LlmClient;

/// A config pointing at `server`, with the LLM enabled.
pub(crate) fn cfg_for(server: &MockServer, model: &str, max_retries: u32) -> LlmConfig {
    LlmConfig {
        enabled: true,
        endpoint: Some(format!("{}/v1", server.uri())),
        model: Some(model.to_owned()),
        api_key: Some("not-needed".to_owned()),
        max_retries,
        ..LlmConfig::default()
    }
}

/// Build an SSE body the SDK will parse into `parts` concatenated.
///
/// The final chunk carries `"finish_reason":"stop"`; without it the SDK emits
/// nothing at all.
pub(crate) fn sse(parts: &[&str]) -> String {
    let mut out = String::new();
    for (i, part) in parts.iter().enumerate() {
        let finish = if i + 1 == parts.len() {
            "\"stop\""
        } else {
            "null"
        };
        out.push_str(&format!(
            "data: {{\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":0,\"model\":\"m\",\"choices\":[{{\"index\":0,\"delta\":{{\"content\":{}}},\"finish_reason\":{finish}}}]}}\n\n",
            serde_json::to_string(part).expect("string serializes")
        ));
    }
    out.push_str("data: [DONE]\n\n");
    out
}

/// How many requests the mock server received.
pub(crate) async fn request_count(server: &MockServer) -> usize {
    // `expect`, not `unwrap_or(0)`. Mapping an unavailable request log to zero
    // makes a broken mock server indistinguishable from one that genuinely
    // received nothing - and the tests that assert "no request was made" are
    // exactly the ones that would then pass for the wrong reason.
    server
        .received_requests()
        .await
        .expect("the mock server must be recording requests")
        .len()
}

/// Build a client through the production `LlmClient::new`, then shrink only the
/// backoff delays so the retry tests do not spend seconds asleep.
///
/// It deliberately does **not** override `max_attempts`. That value comes from
/// `cfg.max_retries` through the production path, and it is the behaviour the
/// retry tests exist to pin. An earlier version took `max_attempts` as a
/// parameter and built `LlmClient` by struct literal, bypassing
/// `LlmClient::new` entirely - forcing `max_attempts = 1` in production left
/// every retry test still passing, including the one asserting the request was
/// retried more than once.
pub(crate) fn fast_retry_client(cfg: &LlmConfig) -> LlmClient {
    let mut client = LlmClient::new(cfg).expect("client builds");
    client.retry_config.initial_delay = Duration::from_millis(10);
    client.retry_config.max_delay = Duration::from_millis(50);
    client.retry_config.jitter_factor = 0.0;
    client
}

/// Mount a 200 SSE response returning `parts`, and hand back the server.
///
/// Every mock in these suites wants the same six lines; stating them once
/// keeps the endpoint path and the content type from drifting between
/// suites.
pub(crate) async fn server_returning(parts: &[&str]) -> MockServer {
    let server = MockServer::start().await;
    mount_sse(
        &server,
        ResponseTemplate::new(200).set_body_raw(sse(parts), "text/event-stream"),
    )
    .await;
    server
}

/// Mount an arbitrary response template at the chat-completions endpoint.
pub(crate) async fn mount_sse(server: &MockServer, template: ResponseTemplate) {
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(template)
        .mount(server)
        .await;
}

/// Mark a file executable on unix; a no-op elsewhere.
///
/// Crate-wide because five copies of this existed across four test modules,
/// each a `#[cfg(unix)]`/`#[cfg(not(unix))]` pair. `expect`, not `unwrap`, so
/// a failure names what went wrong rather than pointing at a line number in a
/// helper.
pub(crate) fn make_executable(path: &std::path::Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(path)
            .expect("the file must exist before its mode is changed")
            .permissions();
        perms.set_mode(perms.mode() | 0o111);
        std::fs::set_permissions(path, perms).expect("setting the executable bit must succeed");
    }
    #[cfg(not(unix))]
    let _ = path;
}

/// Write a `drep.toml` under `dir` pointing at `endpoint`.
///
/// The on-disk config shape stated once. It was written out longhand in three
/// places across two test modules, so the `[llm]` → `[[llm]]` array-of-tables
/// change had to be made three times — and a missed one surfaces not as a
/// failed assertion but as an opaque `ConfigError::Parse` from a test that
/// looks unrelated.
///
/// `max_retries = 1` so a test pointed at a dead endpoint fails on the first
/// attempt rather than paying the SDK's backoff schedule.
pub(crate) fn write_drep_toml(dir: &std::path::Path, endpoint: &str) {
    let body = format!(
        r#"[[llm]]
enabled = true
endpoint = "{endpoint}"
model = "m"
api_key = "not-needed"
max_retries = 1
"#
    );
    std::fs::write(dir.join("drep.toml"), body).expect("drep.toml");
}
