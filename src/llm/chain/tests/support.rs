//! Fixtures specific to the chain suite.
//!
//! Everything that talks to a mock endpoint (`cfg_for`, `sse`, `mount_sse`,
//! `server_returning`, `server_failing_with`, `request_count`,
//! `fast_retry_chain`, `temp_cache`) lives in `crate::test_support`, shared
//! with the client and analysis suites. What is left here is naming: each
//! wrapper below exists for its doc paragraph, not for its body.

use wiremock::MockServer;

use crate::test_support::{server_finishing_with, server_returning};

/// The system prompt and payload every chain test sends. Constants rather than
/// literals at each call site because the cache-key assertions have to name the
/// *same* two strings the request used, and a typo in one of them would make a
/// key comparison pass for the wrong reason.
pub(super) const SYSTEM: &str = "system prompt";
/// See [`SYSTEM`].
pub(super) const CONTENT: &str = "user content";

/// A body the JSON extractor accepts, shaped like a real analyzer response.
pub(super) const GOOD_JSON: &str = r#"{"issues": [], "summary": "clean"}"#;

/// A server that answers 200 with a well-formed but content-free SSE stream.
///
/// The SDK yields zero text here, which drep classifies as a *transport*
/// failure rather than a parse failure - the distinction the empty-body
/// failover rule rests on.
pub(super) async fn server_returning_nothing() -> MockServer {
    server_returning(&[""]).await
}

/// A server that answers 200 with prose containing no JSON at all.
pub(super) async fn server_returning_prose() -> MockServer {
    server_returning(&["I am afraid I cannot help with that."]).await
}

/// A server that answers 200 with a clean analyzer response.
pub(super) async fn server_returning_json() -> MockServer {
    server_returning(&[GOOD_JSON]).await
}

/// A server that answers with prose and reports the output cap.
///
/// Produces `LlmError::ModelStopped`, which is request-shaped: it must neither
/// advance the chain nor demote the provider.
pub(super) async fn server_hitting_the_token_cap() -> MockServer {
    server_finishing_with(&["I will begin by reading"], "length").await
}
