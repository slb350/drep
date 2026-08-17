//! The LLM client.
//!
//! One boundary: `LlmClient::complete_json` takes a system prompt and a user
//! payload, sends them to the configured OpenAI-compatible endpoint, and
//! returns the JSON value the model produced. Cache and concurrency limiting
//! arrive in Phase 3b; this phase owns the request itself.
//!
//! ## What is deliberately delegated to the SDK
//!
//! - **Streaming.** `open-agent-sdk` parses the SSE stream; drep concatenates
//!   the `ContentBlock::Text` blocks it emits and ignores the rest.
//! - **Transport retry.** `retry_with_backoff_conditional` decides per error
//!   whether to retry (5xx, timeout, stream error) or fail fast (4xx, config
//!   errors). drep adds no retry layer on top.
//!
//! ## What this module owns
//!
//! - **Parse retry.** The same prompt truncates the same way, so a parse
//!   failure does NOT retry. The retry closure returns `Ok(None)` for an
//!   unparseable body; the SDK's retry sees `Ok(...)` and stops.
//! - **Attempt count floor.** `LlmConfig::max_retries` may be 0, but a
//!   "zero attempts loop" would skip the request and report a bogus "no
//!   exception was captured". The floor is 1.
//! - **`max_tokens` pass-through.** The configured cap is forwarded only when
//!   the user set one. open-agent-sdk 0.7.0 omits the field entirely otherwise,
//!   so "unset" means the server decides - which is what a 256k-1M context
//!   model needs. (Before 0.7.0 the builder substituted 4096 and truncated
//!   reasoning models mid-thought; drep passed a large sentinel to work around
//!   it. That workaround is gone.)

use std::time::Duration;

use futures::StreamExt;
use open_agent::retry::{RetryConfig, retry_with_backoff_conditional};
use open_agent::{AgentOptions, ContentBlock, query};
use thiserror::Error;

use crate::config::LlmConfig;
use crate::llm::json_parsing::{Extracted, extract_json};

/// A configured LLM client ready to issue requests.
///
/// Built once per process from `LlmConfig`; `complete_json` is the only
/// entry point the analyzer uses.
///
/// Fields are `pub(crate)` so the test submodules can construct clients
/// with a non-default retry config (the production default sleeps 1s
/// between attempts, which would make the retry tests take seconds). They
/// are not part of the public API.
pub struct LlmClient {
    pub(crate) base_url: String,
    pub(crate) model: String,
    pub(crate) api_key: String,
    pub(crate) temperature: f32,
    /// `None` means "no ceiling": since open-agent-sdk 0.7.0 an unset
    /// `max_tokens` is omitted from the request entirely and the server decides.
    /// Before 0.7.0 the builder substituted 4096, which truncated reasoning
    /// models mid-thought, and drep had to pass a large sentinel instead.
    pub(crate) max_tokens: Option<u32>,
    pub(crate) timeout_secs: u64,
    pub(crate) retry_config: RetryConfig,
}

/// Hand-written so the API key cannot reach a log.
///
/// A derived `Debug` prints every field, so any `{:?}`, `dbg!` or tracing line
/// touching the client would emit a live credential.
impl std::fmt::Debug for LlmClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LlmClient")
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field("api_key", &"<redacted>")
            .field("temperature", &self.temperature)
            .field("max_tokens", &self.max_tokens)
            .field("timeout_secs", &self.timeout_secs)
            .finish()
    }
}

/// What can go wrong at the LLM boundary.
///
/// `Transport` and `Unparseable` both mean "the file went unanalyzed". They
/// are distinct so a future caller can decide to retry one but not the other
/// (the spec's split: parse failures are deterministic, transport failures
/// are not). Phase 4 will read this distinction to drive the gating exit
/// code.
#[derive(Debug, Error)]
pub enum LlmError {
    /// Transport failure after the SDK exhausted its retries. The endpoint
    /// was unreachable, timed out, or returned a retryable HTTP error too
    /// many times. The file went unanalyzed.
    #[error("LLM transport failed: {0}")]
    Transport(String),

    /// A response arrived but no JSON could be extracted. Deterministic: do
    /// NOT retry; the same prompt truncates the same way.
    #[error("LLM response was unparseable: {0}")]
    Unparseable(String),

    /// Configuration is incomplete (LLM disabled, no endpoint, no model).
    /// Surfaced at construction so the binary can fail fast instead of
    /// running a gate that will silently never analyze anything.
    #[error("LLM not configured: {0}")]
    NotConfigured(String),
}

impl LlmClient {
    /// Build a client from a validated `LlmConfig`.
    ///
    /// Returns [`LlmError::NotConfigured`] when the config does not name an
    /// endpoint, a model, or has `enabled = false`. We do not default an
    /// endpoint - "LLM was disabled" and "LLM is enabled but misconfigured"
    /// are both fatal here, and inventing a value would mask a broken
    /// install.
    pub fn new(cfg: &LlmConfig) -> Result<Self, LlmError> {
        if !cfg.enabled {
            return Err(LlmError::NotConfigured(
                "LLM is disabled in config (set `enabled = true`)".to_string(),
            ));
        }
        let endpoint = cfg.endpoint.clone().ok_or_else(|| {
            LlmError::NotConfigured("LLM endpoint is not set in config".to_string())
        })?;
        let model = cfg
            .model
            .clone()
            .ok_or_else(|| LlmError::NotConfigured("LLM model is not set in config".to_string()))?;

        let api_key = cfg
            .api_key
            .clone()
            .unwrap_or_else(|| "not-needed".to_string());

        // max_retries is a total attempt count; the floor is 1 so a config of
        // 0 still performs exactly one attempt. See the spec's note on the
        // bogus "no exception was captured" failure.
        let max_attempts = cfg.max_retries.max(1);

        Ok(LlmClient {
            base_url: endpoint,
            model,
            api_key,
            temperature: cfg.temperature,
            max_tokens: cfg.max_tokens,
            timeout_secs: cfg.timeout_secs,
            retry_config: RetryConfig {
                max_attempts,
                initial_delay: Duration::from_secs(1),
                max_delay: Duration::from_secs(60),
                backoff_multiplier: 2.0,
                jitter_factor: 0.1,
            },
        })
    }

    /// Send one prompt and return the extracted JSON.
    ///
    /// Concatenates `ContentBlock::Text` blocks in arrival order; other block
    /// variants are ignored. An empty response body becomes [`LlmError::Unparseable`],
    /// not an empty success - "the model returned nothing" is a deterministic
    /// outcome for the same prompt + max_tokens pair, and the caller must be
    /// able to distinguish it from a clean parse.
    pub async fn complete_json(
        &self,
        system_prompt: &str,
        user_content: &str,
    ) -> Result<Extracted, LlmError> {
        // Build options per request. The SDK doesn't expose a way to set
        // `system_prompt` after `build()`, and the retry closure needs to
        // borrow the same options across attempts.
        let builder = AgentOptions::builder()
            .model(&self.model)
            .base_url(&self.base_url)
            .api_key(&self.api_key)
            .system_prompt(system_prompt)
            .temperature(self.temperature)
            .timeout(self.timeout_secs);

        // Only set a ceiling when the user asked for one. open-agent-sdk 0.7.0
        // omits `max_tokens` from the request when the setter is never called,
        // so "unset" genuinely means "let the server decide" rather than the
        // implicit 4096 earlier versions substituted.
        let builder = match self.max_tokens {
            Some(limit) => builder.max_tokens(limit),
            None => builder,
        };

        let options = builder
            .build()
            .map_err(|e| LlmError::NotConfigured(format!("AgentOptions build failed: {e}")))?;

        let prompt = user_content.to_string();

        // The retry closure returns `Ok(Some(Extracted))` on a clean parse,
        // `Ok(None)` on a successful query that produced no JSON, and
        // `Err(SdkError)` on transport failure. The SDK retries only on
        // `Err`, so unparseable responses do NOT retry - which is the
        // discrimination the spec requires.
        let result: open_agent::Result<Option<Extracted>> =
            retry_with_backoff_conditional(self.retry_config.clone(), || {
                self.run_one_query(&prompt, &options)
            })
            .await;

        match result {
            Ok(Some(extracted)) => Ok(extracted),
            Ok(None) => Err(LlmError::Unparseable(
                "response contained no parseable JSON".to_string(),
            )),
            Err(e) => Err(LlmError::Transport(format!("{e}"))),
        }
    }

    /// One attempt: stream the response, concatenate text, parse.
    ///
    /// Returns `Ok(None)` when the query succeeded but no JSON could be
    /// extracted - this is the path the retry layer must NOT retry.
    /// Returns `Err(SdkError)` for any transport-level failure, which the
    /// retry layer *will* retry when the SDK classifies it as transient.
    async fn run_one_query(
        &self,
        prompt: &str,
        options: &AgentOptions,
    ) -> open_agent::Result<Option<Extracted>> {
        let mut stream = query(prompt, options).await?;
        let mut text = String::new();
        while let Some(block) = stream.next().await {
            // Image, ToolUse, ToolResult are not used here.
            if let ContentBlock::Text(t) = block? {
                text.push_str(&t.text);
            }
        }
        Ok(extract_json(&text))
    }
}

#[cfg(test)]
mod tests;
