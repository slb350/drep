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
//!   the `ContentBlock::Text` blocks it emits and ignores the rest. Since
//!   0.10.0 those blocks are *fragments* - one event per delta, delivered
//!   while the stream is open, where 0.9.x emitted the whole response as a
//!   single block at the end. The types are identical either way, so nothing
//!   here failed to compile and nothing here changed: the join in
//!   `run_one_query` is what makes the assembled text independent of where
//!   the deltas fall. Reading one block as the whole answer would now return a
//!   prefix, and `src/llm/client/tests/streaming.rs` is what would notice.
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
use open_agent::{AgentOptions, ApiProtocol, ContentBlock, FinishReason, StreamEvent, query};
use thiserror::Error;

use crate::config::LlmConfig;
use crate::llm::json_parsing::{Extracted, extract_json};
use crate::text::excerpt;

/// How much of a model response reaches an error message.
///
/// Generous: unlike a URL, the useful signal in a refusal or a prose preamble
/// is often a sentence or two in.
const RESPONSE_EXCERPT_MAX: usize = 200;

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
    /// The wire protocol this endpoint speaks. Selects the request path, the auth
    /// header, the body shape and the streaming vocabulary together - the SDK
    /// resolves all four from this one value.
    pub(crate) protocol: ApiProtocol,
    /// `None` sends no `temperature` at all. Two of the four models drep ships a
    /// preset for reject the parameter outright, and a 400 neither fails over nor
    /// retries, so "omit it" had to be expressible rather than approximated by a
    /// low value.
    pub(crate) temperature: Option<f32>,
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
            .field("protocol", &self.protocol.as_str())
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
///
/// `Clone` because the provider chain records the reason a provider went down
/// and hands a copy to every later file that skips it. The variants are three
/// owned `String`s and an `Option<u16>`; there is nothing here a clone can get
/// wrong.
#[derive(Debug, Clone, Error)]
pub enum LlmError {
    /// Transport failure after the SDK exhausted its retries. The endpoint
    /// was unreachable, timed out, or returned a retryable HTTP error too
    /// many times. The file went unanalyzed.
    ///
    /// `status` is the HTTP code when the SDK surfaced one (via
    /// `open_agent::Error::status_code`); `None` for spawn/timeouts, which
    /// never have one. Keeping the code as a number rather than only inside
    /// the message is what lets a caller branch on the value — a 429 is
    /// meaningfully different from a 500.
    #[error("LLM transport failed{}: {message}", status.map(|c| format!(" (HTTP {c})")).unwrap_or_default())]
    Transport {
        status: Option<u16>,
        message: String,
    },

    /// A response arrived but no JSON could be extracted. Deterministic: do
    /// NOT retry; the same prompt truncates the same way.
    #[error("LLM response was unparseable: {0}")]
    Unparseable(String),

    /// The model stopped before producing any JSON, and the server said why.
    ///
    /// Deterministic in a way [`Self::Unparseable`] is not: the request hit a
    /// limit, so asking again hits the same one. `finish` is the server's own
    /// word for it, kept as a machine tag beside the human `message` - the same
    /// shape as [`Self::Transport`]'s `status`, and for the same reason.
    ///
    /// It is about the *request*, never the endpoint, so it must not fail over
    /// and must not demote the provider. A second provider cannot make a file
    /// smaller.
    #[error("{message}")]
    ModelStopped { finish: String, message: String },

    /// Configuration is incomplete (LLM disabled, no endpoint, no model).
    /// Surfaced at construction so the binary can fail fast instead of
    /// running a gate that will silently never analyze anything.
    #[error("LLM not configured: {0}")]
    NotConfigured(String),
}

impl LlmError {
    /// The HTTP status, when the failure carried one.
    ///
    /// Only `Transport` ever does. It exists so the tests that pin the
    /// failover policy can name a status rather than substring-match the
    /// message - the message is prose and prose gets reworded. `should_failover`
    /// itself matches the variant, because it also has to distinguish
    /// `Unparseable` from a status-less transport failure, which a bare
    /// `Option<u16>` cannot.
    pub fn status(&self) -> Option<u16> {
        match self {
            LlmError::Transport { status, .. } => *status,
            LlmError::Unparseable(_)
            | LlmError::ModelStopped { .. }
            | LlmError::NotConfigured(_) => None,
        }
    }
}

impl LlmClient {
    /// The model this client asks for.
    ///
    /// An accessor rather than a second copy on the caller: the cache key is
    /// computed from the model, and a struct holding its own `model` string is
    /// exactly what lets a request go to one model while the key names
    /// another.
    pub fn model(&self) -> &str {
        &self.model
    }

    /// The base URL this client talks to. For display - `doctor` and the
    /// failover report name the endpoint a provider used.
    pub fn endpoint(&self) -> &str {
        &self.base_url
    }

    /// The sampling temperature, or `None` when none is sent. Part of the cache
    /// key, for the same reason [`Self::model`] is - and `None` has to key
    /// differently from any value, because the answers genuinely differ.
    pub fn temperature(&self) -> Option<f32> {
        self.temperature
    }

    /// The wire protocol this client speaks. For display, and for the cache key:
    /// the same model at the same endpoint over two protocols is two requests.
    pub fn protocol(&self) -> ApiProtocol {
        self.protocol
    }

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

        // `config::load` already rejected an unknown name, so this cannot fail for a
        // config that came through the loader. It is re-checked rather than unwrapped
        // because `LlmClient::new` is also reachable from tests that build an
        // `LlmConfig` directly, and a silent default here would post
        // chat-completions bytes to a `/messages` endpoint.
        let protocol = crate::config::parse_protocol(cfg.protocol.as_deref()).ok_or_else(|| {
            LlmError::NotConfigured(format!(
                "unknown protocol `{}`; expected `openai` or `anthropic`",
                cfg.protocol.as_deref().unwrap_or_default()
            ))
        })?;

        // max_retries is a total attempt count; the floor is 1 so a config of
        // 0 still performs exactly one attempt. See the spec's note on the
        // bogus "no exception was captured" failure.
        let max_attempts = cfg.max_retries.max(1);

        Ok(LlmClient {
            base_url: endpoint,
            model,
            api_key,
            protocol,
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
    /// variants are ignored. An empty response body is **retried** as a
    /// transport failure and, if it keeps coming back empty, surfaces as
    /// [`LlmError::Transport`] - see `run_one_query` for why "the model
    /// returned nothing" is not the deterministic outcome it looks like.
    ///
    /// A non-empty body that yields **no JSON at all** is retried up to
    /// [`NO_JSON_ATTEMPTS`] times and then becomes [`LlmError::Unparseable`],
    /// carrying an excerpt of what actually came back. A body that parsed only
    /// after brace-balancing ([`Extracted::Truncated`]) is returned
    /// immediately and never retried - that is the genuinely deterministic
    /// case, and the one the "never retry" rule was written for.
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
            .protocol(self.protocol)
            .timeout(self.timeout_secs);

        // Only send a temperature when one was configured. An unset value means the
        // field is omitted entirely, which is the only thing that works against a
        // model that rejects the parameter.
        let builder = match self.temperature {
            Some(temperature) => builder.temperature(temperature),
            None => builder,
        };

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

        // The no-JSON retry is drep's own loop, deliberately *outside* the
        // SDK's. Handing "no JSON" to the SDK by returning `Err` would work,
        // but it would surface as `LlmError::Transport` once the attempts ran
        // out - and `Transport` fails over to the next provider and demotes
        // this one for the whole run. A model that answered in prose has told
        // us nothing about the endpoint, so neither of those is right.
        //
        // The SDK's own retry still runs inside each pass, so a transport
        // failure is handled by the layer that classifies it.
        let mut last_body = String::new();
        for _ in 0..NO_JSON_ATTEMPTS {
            let result: open_agent::Result<Answer> =
                retry_with_backoff_conditional(self.retry_config.clone(), || {
                    self.run_one_query(&prompt, &options)
                })
                .await;

            match result {
                Ok(Answer::Parsed(extracted)) => return Ok(extracted),
                // The server said why it stopped, and the reason rules out a
                // retry: the request hit a limit, so the same request hits the
                // same limit. This is the genuinely deterministic case the
                // original "never retry a non-empty body" rule was reaching
                // for - it just used "no JSON in the body" as the proxy, which
                // is not the same question.
                Ok(Answer::NoJson { text, finish }) if !worth_asking_again(&finish) => {
                    return Err(LlmError::ModelStopped {
                        finish: finish.as_str().to_owned(),
                        message: stopped_message(&finish, &text),
                    });
                }
                Ok(Answer::NoJson { text, .. }) => last_body = text,
                Err(e) => {
                    // The SDK exposes the status code separately (via
                    // `status_code`); reading it before formatting means the
                    // number survives as a number, and a later caller can
                    // branch on it rather than parsing the message.
                    let status = e.status_code();
                    let message = format!("{e}");
                    return Err(LlmError::Transport { status, message });
                }
            }
        }

        Err(LlmError::Unparseable(format!(
            "no JSON in the response after {NO_JSON_ATTEMPTS} attempts; \
             the model answered: {}",
            excerpt(&last_body, RESPONSE_EXCERPT_MAX)
        )))
    }

    /// One attempt: stream the response, concatenate text, parse.
    ///
    /// Returns [`Answer::NoJson`] carrying the raw text when the query
    /// produced something we could not parse at all - the SDK's retry layer
    /// sees `Ok` and stops, leaving the decision to `complete_json`. Returns
    /// `Err(SdkError)` for any transport-level failure, *including an empty
    /// response*, which the retry layer will retry when the SDK classifies it
    /// as transient.
    async fn run_one_query(
        &self,
        prompt: &str,
        options: &AgentOptions,
    ) -> open_agent::Result<Answer> {
        let mut stream = query(prompt, options).await?;
        let mut text = String::new();
        // `Unspecified` is the right default rather than a panic-if-absent:
        // several OpenAI-compatible servers never report a reason at all, and
        // "no information" is a distinct answer from "stopped normally".
        let mut finish = FinishReason::Unspecified;
        while let Some(event) = stream.next().await {
            match event? {
                // Image, ToolUse, ToolResult are not used here.
                StreamEvent::Block(ContentBlock::Text(t)) => text.push_str(&t.text),
                StreamEvent::Finish(reason) => finish = reason,
                // Everything else is discarded, and that is the contract:
                // `text` holds assistant text and nothing else. It covers the
                // non-text blocks drep has no use for, the `Reasoning` side
                // channel (opt-in, and drep does not opt in - chain-of-thought
                // must never reach the text drep parses as JSON), and any
                // variant a later SDK adds, since `StreamEvent` is
                // `#[non_exhaustive]`. Spelled as one arm because a separate
                // `Reasoning(_) => {}` above it does the same nothing, and an
                // arm indistinguishable from the wildcard is dead code.
                _ => {}
            }
        }

        // An empty body is a **transport** failure, not a parse failure.
        //
        // This distinction was learned the expensive way. Both cases used to
        // return `Ok(None)` and become a non-retrying `Unparseable`, on the
        // stated reasoning that "the model returned nothing" repeats
        // deterministically for the same prompt. It does not: on drep's own
        // first gated push, 7 of 49 files came back with no parseable JSON,
        // and re-running one of them immediately afterwards succeeded with
        // findings and exit 0. The provider had simply returned nothing that
        // time - which is exactly the `finish_reason='error'` flakiness that
        // blocked three consecutive pushes under 1.x.
        //
        // `Error::stream` is classified retryable by the SDK, which is both
        // accurate (the stream completed carrying no content) and nearly free:
        // a response with no output tokens cost nothing to produce, so asking
        // again is cheap. A *non-empty* body we cannot parse still returns
        // `Ok(None)` and still does not retry - that is the deterministic case
        // the split was built for, and re-sending it burns a full reasoning
        // call for the same answer.
        if text.trim().is_empty() {
            return Err(open_agent::Error::stream(
                "the model returned an empty response",
            ));
        }

        Ok(match extract_json(&text) {
            Some(extracted) => Answer::Parsed(extracted),
            // The text is carried out rather than dropped. It was discarded
            // behind the constant "response contained no parseable JSON",
            // which made every occurrence of this failure look identical and
            // left no way to tell a refusal from a prose preamble from
            // reasoning that leaked into the content channel.
            None => Answer::NoJson { text, finish },
        })
    }
}

/// What one query produced, before the retry decision.
///
/// `NoJson` carries the body so the failure can be diagnosed and so
/// `complete_json` can decide whether to ask again. The SDK's retry layer
/// treats both variants as success and stops, which is what keeps the
/// no-JSON decision here rather than inside it.
enum Answer {
    Parsed(Extracted),
    /// No JSON at all, with why generation stopped. The reason decides whether
    /// asking again can possibly help.
    NoJson {
        text: String,
        finish: FinishReason,
    },
}

/// How many times a response carrying no JSON at all is asked for again.
///
/// Not the same question as the SDK's transport retry, and deliberately a
/// small number: each attempt is a full reasoning call. The rule this replaced
/// never retried, justified as "the same prompt truncates the same way" - but
/// that is [`Extracted::Truncated`], a different branch. A response with *no
/// JSON at all* did not truncate an answer, it never produced one, and in
/// practice it does not repeat: drep's own gated push failed on a different
/// file each run, and each failing file analyzed cleanly when asked again.
///
/// Two attempts, so one retry. The evidence is that a single retry clears it,
/// and a model that answers in prose twice is not going to be talked round on
/// the third try.
pub const NO_JSON_ATTEMPTS: u32 = 2;

/// Whether asking the same question again could produce a different answer.
///
/// `false` for the reasons that are a property of the *request*: a token cap is
/// hit identically every time, and a content filter that refused this payload
/// refuses it again. `true` where the server told us nothing useful, because a
/// model at temperature above zero can simply answer differently - which is
/// what drep's own gated push demonstrated, failing on a different file each
/// run with every failing file analyzing cleanly when asked again.
fn worth_asking_again(finish: &FinishReason) -> bool {
    // Written as a negated match on the two request-shaped reasons rather than
    // as an enumeration of the rest. `FinishReason` is `#[non_exhaustive]`, so
    // a wildcard arm is required either way - and an enumerated "everything
    // else is retryable" arm sitting above it is behaviourally identical to the
    // wildcard, which makes it undeletable-but-unobservable: exactly the dead
    // code the mutation gate exists to find.
    //
    // The consequence of the wildcard is deliberate: a reason a later SDK adds
    // defaults to retrying. The retry is bounded and cheap to be wrong about,
    // whereas refusing to retry something transient fails a commit outright.
    !matches!(
        finish,
        // A token cap is hit identically every time - drep sends no
        // `max_tokens`, so the cap is the server's. A content filter that
        // refused this payload refuses it again.
        FinishReason::Length | FinishReason::ContentFilter
    )
}

/// A sentence a user can act on, for the reasons that end the attempt.
///
/// The two cases want different actions - one is "this file is too big for this
/// model in one pass", the other is "this provider refused the content" - so
/// they do not share a message.
fn stopped_message(finish: &FinishReason, text: &str) -> String {
    match finish {
        FinishReason::Length => format!(
            "the model hit its output token limit before producing any JSON. \
             This file is too large for this model to review in one request - \
             split it, or use a provider with a larger output budget. \
             It managed: {}",
            excerpt(text, RESPONSE_EXCERPT_MAX)
        ),
        _ => format!(
            "the model stopped ({}) before producing any JSON: {}",
            finish.as_str(),
            excerpt(text, RESPONSE_EXCERPT_MAX)
        ),
    }
}

#[cfg(test)]
mod tests;
