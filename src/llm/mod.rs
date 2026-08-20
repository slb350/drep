//! LLM client and JSON extraction.
//!
//! Five sub-modules, split by concern rather than by phase:
//!
//! - [`json_parsing`] turns whatever the model returned into a `serde_json::Value`.
//!   Tolerant of fences, prose, trailing commas and truncated output, in that
//!   order, first success wins.
//! - [`client`] wraps `open-agent-sdk`. The request itself - streaming,
//!   transport retry, and parse retry.
//! - [`cache`] is the on-disk response cache. Content-addressed keys (not
//!   git-aware), infallible reads, oldest-first eviction.
//! - [`concurrency`] is the bounded in-flight limiter. Deliberately just a
//!   `Semaphore`; the rate-limit machinery the Python carried is gone (see
//!   the module doc for the four reasons).
//! - [`chain`] is the ordered list of providers with failover. It owns the
//!   loop, so the cache key is recomputed per provider and the answer is
//!   filed under the key of whoever gave it.
//!
//! `chain::ProviderChain::complete_json` is the boundary the analyzer calls;
//! `client::LlmClient::complete_json` is the single-provider request beneath
//! it. It
//! returns the `Extracted` from `json_parsing`, never the raw text, so a
//! truncated response stays a type the caller can pattern-match on rather
//! than a log line to grep for.

pub mod cache;
pub mod chain;
pub mod client;
pub mod concurrency;
pub mod json_parsing;
pub mod models;
