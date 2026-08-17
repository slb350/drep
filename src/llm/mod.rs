//! LLM client and JSON extraction.
//!
//! Three sub-modules, split by concern rather than by phase:
//!
//! - [`json_parsing`] turns whatever the model returned into a `serde_json::Value`.
//!   Tolerant of fences, prose, trailing commas and truncated output, in that
//!   order, first success wins.
//! - [`client`] wraps `open-agent-sdk`. Cache and concurrency limiting arrive
//!   in Phase 3b; this phase owns the request itself.
//!
//! `client::LlmClient::complete_json` is the boundary the analyzer calls. It
//! returns the `Extracted` from `json_parsing`, never the raw text, so a
//! truncated response stays a type the caller can pattern-match on rather
//! than a log line to grep for.

pub mod client;
pub mod json_parsing;
