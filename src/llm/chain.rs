//! The provider chain: an ordered list of LLM providers, tried in turn.
//!
//! drep 2.0 dropped the circuit breaker and the rate limiter as server-shaped
//! complexity. Failover is the same category of machinery and is taken anyway,
//! for one specific failure that happens in practice: a local endpoint that is
//! off blocks **every** commit, because "could not analyze" is a hard stop by
//! design. The bar it had to clear is recorded in `docs/rust-migration.md`
//! rather than left implicit here.
//!
//! ## Two independent questions about a failure
//!
//! - [`should_failover`] — does the next provider get asked? Only *transport*
//!   failures advance the chain: the SDK's retryable set (408, 429, 5xx) plus
//!   the status-less failures (timeout, connection refused, an empty body). A
//!   401 or 403 must **not**: that is misconfiguration, and quietly asking a
//!   second provider masks it. An unparseable-but-non-empty response must not
//!   either — it is deterministic, so a retry anywhere buys nothing.
//! - [`is_sticky`] — is the failure remembered for the rest of the run? Every
//!   failure that advances the chain, plus the two that are a property of the
//!   connection rather than the request: a stale API key returns 401 for every
//!   file, and re-handshaking forty-nine times to be told so again is pure
//!   wall-clock on a commit gate that is going to exit 2 anyway. A *request*
//!   -level 4xx is deliberately excluded - remembering one would let a single
//!   oversized payload stop the chain for every later file.
//!
//! Conflating the two is what makes a chain either mask a bad key or re-ask a
//! dead endpoint once per file. A remembered failure is then replayed through
//! `should_failover` exactly as a live one would be, so a demoted 401 still
//! stops the chain and a demoted 500 still advances it.
//!
//! An **empty** response does fail over. It reaches here as
//! `Transport { status: None }` only after the SDK has already retried it, and
//! a provider that keeps answering with nothing is as unusable as one that is
//! down - which is exactly the flakiness that failed 7 of 49 files on drep's
//! own first gated push. It also produced zero output tokens, so the attempts
//! it burned cost almost nothing.
//!
//! ## The cache key moves with the provider
//!
//! The key is computed from the provider's *endpoint and* model, so it is
//! computed **inside** the loop, once per provider tried. Keying provider 1 and
//! then letting provider 2 serve the answer would file that answer under a key
//! it did not come from, and a later run with provider 1 healthy would get a
//! hit that never came from provider 1. [`Served::key`] is the key of the
//! provider that actually answered; the caller stores under that key or not at
//! all.
//!
//! The endpoint is in the key because a model name is not an identity: one open
//! model served locally and from a cloud provider is the canonical failover
//! pair, and both name it the same thing.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::config::LlmConfig;
use crate::llm::cache::{Cache, CacheKey};
use crate::llm::client::{LlmClient, LlmError};
use crate::llm::concurrency::Limiter;
use crate::llm::json_parsing::Extracted;

/// One provider: a client, the concurrency budget for *its* endpoint, and the
/// two pieces of run-scoped state that belong to it.
///
/// The limiter is per provider rather than per process because the slot
/// represents in-flight HTTP work against one endpoint, and `max_concurrent`
/// is configured per `[[llm]]` entry. A single shared limiter would apply the
/// local model's generous budget to a rate-limited cloud endpoint.
///
/// `down` and `served` live here rather than in vectors parallel to the
/// provider list. Parallel vectors force a length invariant the type system
/// cannot check, and make every reader defensive about an index that is
/// structurally always valid — a `mark_down` that silently did nothing on an
/// out-of-range index would be invisible.
///
/// Model and endpoint are **not** duplicated onto this struct; they are read
/// from the client, which is the same rule `CodeQualityAnalyzer` follows. A
/// second copy is what lets a request go to one model while the cache key
/// names another.
#[derive(Debug)]
pub struct Provider {
    /// `pub(crate)` for the same reason `LlmClient`'s fields are: the test
    /// fixtures build a chain through the production `ProviderChain::new` and
    /// then shrink only the backoff delays, so a retry test does not spend
    /// seconds asleep. Not part of the public API.
    pub(crate) client: LlmClient,
    limiter: Limiter,
    /// Why this provider was demoted, set once. `OnceLock` rather than a
    /// `Mutex<Option<_>>` because the value is write-once and read-often, and
    /// first-writer-wins is the semantics we want anyway — concurrent files
    /// hitting the same dead endpoint should record the first reason, not race
    /// to overwrite it.
    down: OnceLock<LlmError>,
    /// How many files this provider answered, cache hits included.
    served: AtomicUsize,
}

impl Provider {
    /// The model this provider asks for.
    pub fn model(&self) -> &str {
        self.client.model()
    }

    /// The base URL this provider talks to.
    pub fn endpoint(&self) -> &str {
        self.client.endpoint()
    }

    /// The concurrency budget for this provider's endpoint.
    ///
    /// Exposed because it is the only thing that can demonstrate the limiter
    /// works: wiremock never overlaps requests, so neither an in-flight
    /// counter nor wall-clock can tell a working limiter from a deleted one.
    /// Observing `available()` while the analysis runs can.
    pub fn limiter(&self) -> &Limiter {
        &self.limiter
    }

    /// How many files this provider answered.
    ///
    /// Counted here rather than carried out through the analysis result and
    /// rejoined against the chain later: the chain already knows who answered,
    /// it is already shared for the whole process, and it already holds
    /// per-provider interior-mutable state. A cache hit counts — the answer
    /// still originated with that provider, and the user's code was still
    /// reviewed by that model.
    pub fn served(&self) -> usize {
        self.served.load(Ordering::Relaxed)
    }

    /// Whether this provider has been demoted for the rest of the run.
    pub fn is_down(&self) -> bool {
        self.down.get().is_some()
    }

    /// Why it was demoted, if it was.
    fn down_reason(&self) -> Option<&LlmError> {
        self.down.get()
    }

    /// Demote for the rest of the run, recording why. First writer wins; a
    /// concurrent second failure is dropped rather than overwriting the reason
    /// a user is about to read.
    fn mark_down(&self, err: LlmError) {
        let _ = self.down.set(err);
    }

    fn record_served(&self) {
        self.served.fetch_add(1, Ordering::Relaxed);
    }

    /// This provider's cache key for one prompt.
    ///
    /// The single definition of "which key belongs to which provider". It lives
    /// here rather than at the call site so a test cannot compute the key a
    /// different way than production does - which is exactly how the endpoint
    /// went missing from it: the tests spelled out `model` and `temperature` by
    /// hand and agreed with the bug.
    pub fn cache_key(&self, cache: &Cache, system_prompt: &str, user_content: &str) -> CacheKey {
        cache.key(
            system_prompt,
            user_content,
            self.endpoint(),
            self.model(),
            self.client.temperature(),
        )
    }
}

/// One provider's contribution to a failed file.
///
/// `skipped` distinguishes "tried now and failed" from "already down from
/// earlier in this run". Both are worth reporting: a user reading why a file
/// went unanalyzed needs to know the local endpoint has been dead since file
/// three, not just that the cloud fallback then returned a 401.
#[derive(Debug)]
pub struct Attempt {
    /// Zero-based position in the chain. Rendered one-based.
    pub provider: usize,
    /// The model that provider asks for, for a message the user can act on.
    pub model: String,
    /// Why it failed.
    pub error: LlmError,
    /// True when this provider was already marked down and was not contacted
    /// for this file.
    pub skipped: bool,
}

impl Attempt {
    fn new(index: usize, provider: &Provider, error: LlmError, skipped: bool) -> Self {
        Self {
            provider: index,
            model: provider.model().to_owned(),
            error,
            skipped,
        }
    }
}

/// No provider produced an answer, with what each one contributed.
///
/// `attempts` is never empty: [`ProviderChain::new`] rejects an empty chain, so
/// a file that could not be analyzed always names at least one provider. It can
/// be *shorter* than `chain_len` — a 401 at the head stops the chain, and the
/// providers below it were never consulted.
///
/// `chain_len` is carried so the caller can tell "the only provider failed"
/// from "a chain stopped early", which are the same `attempts.len() == 1` but
/// very different questions for the user.
#[derive(Debug)]
pub struct ChainError {
    pub attempts: Vec<Attempt>,
    pub chain_len: usize,
}

/// A response, and which provider produced it.
///
/// `key` is that provider's cache key - the caller stores under it, so the
/// entry is filed against the model that actually answered.
#[derive(Debug)]
pub struct Served {
    /// Zero-based position in the chain.
    pub provider: usize,
    /// The cache key of the provider that answered.
    pub key: CacheKey,
    /// What came back.
    pub extracted: Extracted,
    /// True when the cache answered and no request was made.
    pub from_cache: bool,
}

/// An ordered chain of providers with sticky demotion.
///
/// Built once per process from `Config::providers()` and shared across the
/// analyzer, so the demotion one file discovers is visible to every other.
#[derive(Debug)]
pub struct ProviderChain {
    /// `pub(crate)` so the test fixtures can shrink the per-provider backoff.
    /// See [`Provider`].
    pub(crate) providers: Vec<Provider>,
}

impl ProviderChain {
    /// Build a chain from the enabled providers, in order.
    ///
    /// A misconfigured entry is fatal rather than skipped. Skipping it would
    /// be the same masking that the 401 rule forbids: an endpoint-less
    /// `[[llm]]` block is a broken install, and a gate that quietly routes
    /// around it is a gate reporting on a configuration the user did not
    /// write. The index is carried into the message because with several
    /// providers "LLM model is not set" does not say which one.
    pub fn new(cfgs: &[&LlmConfig]) -> Result<Self, LlmError> {
        if cfgs.is_empty() {
            return Err(LlmError::NotConfigured(
                "no enabled `[[llm]]` provider".to_string(),
            ));
        }
        let mut providers = Vec::with_capacity(cfgs.len());
        for (index, cfg) in cfgs.iter().enumerate() {
            let client = LlmClient::new(cfg)
                .map_err(|err| LlmError::NotConfigured(format!("[[llm]] #{}: {err}", index + 1)))?;
            providers.push(Provider {
                client,
                limiter: Limiter::new(cfg.max_concurrent),
                down: OnceLock::new(),
                served: AtomicUsize::new(0),
            });
        }
        Ok(Self { providers })
    }

    /// The providers, in preference order.
    pub fn providers(&self) -> &[Provider] {
        &self.providers
    }

    /// Send one prompt down the chain, consulting the cache per provider.
    ///
    /// Returns the first answer anyone gives, or every provider's reason for
    /// not giving one.
    pub async fn complete_json(
        &self,
        system_prompt: &str,
        user_content: &str,
        cache: &Cache,
    ) -> Result<Served, ChainError> {
        let mut attempts: Vec<Attempt> = Vec::new();

        for (index, provider) in self.providers.iter().enumerate() {
            match try_provider(index, provider, system_prompt, user_content, cache).await {
                ProviderOutcome::Served(served) => return Ok(served),
                // One place decides what a failure means for the loop, so the
                // three ways a provider can fail cannot disagree about it.
                ProviderOutcome::Failed { attempt, advance } => {
                    attempts.push(attempt);
                    if !advance {
                        return Err(self.error(attempts));
                    }
                }
            }
        }

        Err(self.error(attempts))
    }

    fn error(&self, attempts: Vec<Attempt>) -> ChainError {
        ChainError {
            attempts,
            chain_len: self.providers.len(),
        }
    }
}

/// What one provider did with the request.
enum ProviderOutcome {
    Served(Served),
    Failed { attempt: Attempt, advance: bool },
}

impl ProviderOutcome {
    /// A provider skipped because it was already demoted.
    ///
    /// The recorded reason is replayed through [`should_failover`] exactly as a
    /// live failure would be, so a remembered 401 stops the chain here and a
    /// remembered 500 advances it. Deciding otherwise would let a bad key be
    /// routed around on every file after the first.
    fn demoted(index: usize, provider: &Provider, err: &LlmError) -> Self {
        ProviderOutcome::Failed {
            advance: should_failover(err),
            attempt: Attempt::new(index, provider, err.clone(), true),
        }
    }
}

/// Try one provider: demotion check, cache, limiter, request.
///
/// A free function rather than a method because it reads no chain state — all
/// of it now lives on the `Provider` it is handed.
async fn try_provider(
    index: usize,
    provider: &Provider,
    system_prompt: &str,
    user_content: &str,
    cache: &Cache,
) -> ProviderOutcome {
    // Already down: report what it said the first time rather than paying the
    // SDK's backoff schedule to be told again.
    if let Some(err) = provider.down_reason() {
        return ProviderOutcome::demoted(index, provider, err);
    }

    // The key is computed here, from *this* provider's model, and the `Served`
    // carries it back so the caller cannot file the answer under a different
    // provider's key.
    let key = provider.cache_key(cache, system_prompt, user_content);
    if let Some(value) = cache.get(&key) {
        provider.record_served();
        return ProviderOutcome::Served(Served {
            provider: index,
            key,
            extracted: Extracted::Complete(value),
            from_cache: true,
        });
    }

    // The slot is held for the request and released on every exit path,
    // failover included. The cache hit above never takes one: the slot
    // represents in-flight HTTP work, and a cache read is not in flight.
    let guard = provider.limiter.acquire().await;

    // Re-check after waiting. Files are analyzed concurrently, so the check
    // above can only stop files that had not yet started; everything already
    // queued on this provider's limiter passed it before the first failure
    // landed. Without this second look, forty-nine files queued against a dead
    // endpoint each pay the SDK's full retry schedule anyway, and sticky
    // demotion saves nothing in the one case it exists for. With it, the waste
    // is bounded by `max_concurrent` rather than by the number of files.
    if let Some(err) = provider.down_reason() {
        drop(guard);
        return ProviderOutcome::demoted(index, provider, err);
    }

    let outcome = provider
        .client
        .complete_json(system_prompt, user_content)
        .await;
    drop(guard);

    match outcome {
        Ok(extracted) => {
            provider.record_served();
            ProviderOutcome::Served(Served {
                provider: index,
                key,
                extracted,
                from_cache: false,
            })
        }
        Err(err) => {
            if is_sticky(&err) {
                provider.mark_down(err.clone());
            }
            ProviderOutcome::Failed {
                advance: should_failover(&err),
                attempt: Attempt::new(index, provider, err, false),
            }
        }
    }
}

/// Whether this failure should be handed to the next provider.
///
/// The whole failover policy, in one place, so the rule cannot be restated
/// differently at a second site.
fn should_failover(err: &LlmError) -> bool {
    match err {
        // No status: a timeout, a refused connection, or an empty body. All
        // endpoint-level, all worth asking someone else.
        LlmError::Transport { status: None, .. } => true,
        LlmError::Transport {
            status: Some(code), ..
        } => is_retryable_status(*code),
        // A non-empty body we could not parse is deterministic - a second
        // provider is a second full-price call for the same outcome.
        LlmError::Unparseable(_) => false,
        // Misconfiguration. Routing around it is what hides it.
        LlmError::NotConfigured(_) => false,
    }
}

/// Whether this failure is remembered for the rest of the run.
///
/// Deliberately a wider set than [`should_failover`]. A 401 does not advance
/// the chain, but it is still a property of the endpoint rather than of this
/// file: every file in the run will get the same answer, so ask once.
fn is_sticky(err: &LlmError) -> bool {
    // Remember a failure only when remembering it cannot change a later file's
    // outcome. Two ways that holds:
    //
    // - The chain advances past this provider anyway, so skipping it costs the
    //   later file nothing it was not already going to pay.
    // - It is a credential the endpoint rejects, which it will reject for every
    //   request regardless of payload.
    //
    // The combination to avoid is "remembered" *and* "does not fail over": a
    // later file skips the provider, replays the stored reason, and stops the
    // chain without contacting anyone. `Transport { .. } => true` had exactly
    // that shape for a request-level 4xx - one oversized payload drawing a 400
    // demoted the provider, and every file after it stopped there without ever
    // reaching the configured fallback. One bad file poisoned the run.
    should_failover(err) || is_auth_failure(err)
}

/// Whether the endpoint rejected the credential rather than the request.
///
/// 401 and 403 are the two statuses that are a property of the *connection* and
/// not of what was sent, so they are the only non-failover failures worth
/// remembering: a stale key answers the same way for every file, and
/// re-handshaking once per file is pure wall-clock on a gate that will exit 2
/// regardless.
fn is_auth_failure(err: &LlmError) -> bool {
    matches!(
        err,
        LlmError::Transport {
            status: Some(401 | 403),
            ..
        }
    )
}

/// The retryable HTTP statuses.
///
/// 408 and 429 are the two 4xx codes that mean "ask again"; everything else in
/// the 4xx range is the client's fault and a second provider cannot fix it.
/// 5xx is the server's fault and another server might not have it.
///
/// Deliberately drep's own list rather than a claim to mirror the SDK's:
/// open-agent-sdk's retryable set is private, and it excludes some 5xx codes
/// (501, 505) that a *different provider* may well not return at all. The two
/// answer different questions - "retry this endpoint" and "try another one".
fn is_retryable_status(code: u16) -> bool {
    matches!(code, 408 | 429) || (500..=599).contains(&code)
}

#[cfg(test)]
mod tests;
