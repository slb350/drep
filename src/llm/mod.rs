//! LLM client and JSON extraction.
//!
//! Four sub-modules, split by concern rather than by phase:
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
//!
//! `client::LlmClient::complete_json` is the boundary the analyzer calls. It
//! returns the `Extracted` from `json_parsing`, never the raw text, so a
//! truncated response stays a type the caller can pattern-match on rather
//! than a log line to grep for.

pub mod cache;
pub mod client;
pub mod concurrency;
pub mod json_parsing;
