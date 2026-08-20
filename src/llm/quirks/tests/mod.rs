//! Unit tests for the model-quirks registry.
//!
//! Wired in via `#[cfg(test)] mod tests;` in `quirks.rs`. Every file here must
//! be declared below: Rust silently ignores a file no `mod` points at, which
//! once left four files of tests uncompiled in this repository while the count
//! still looked right.
//!
//! Nothing here reaches models.dev. The document is a literal and the network
//! is a [`Canned`] stub, for the same reason the wizard's tests inject a
//! catalogue: a suite that fetched 4 MB per test would be slow, offline-hostile
//! and dependent on somebody else's uptime.

mod cache;
mod distil;
mod http;
mod resolve;

use super::{Fetch, QuirksError};

/// The shared models.dev fixture. Re-exported under the name these files have
/// always used; it lives in `test_support` because the wizard's tests need the
/// same document, and two copies of it disagreed about whether `glm-5.3`
/// accepts a temperature.
pub(crate) use crate::test_support::MODELS_DEV_DOCUMENT as DOCUMENT;

/// A document whose providers all lack an endpoint, so nothing can be joined.
pub(crate) const NO_ENDPOINTS: &str = r#"{
  "anthropic": { "id": "anthropic", "api": null, "models": { "fable-5": { "id": "fable-5" } } }
}"#;

/// A [`Fetch`] that answers from memory and counts how often it was asked.
///
/// The count is what separates "the cache was used" from "the cache was
/// ignored and the answer happened to match", which is the whole behaviour
/// [`super::Cached`] exists for.
pub(crate) struct Canned {
    document: Option<String>,
    pub calls: std::cell::Cell<usize>,
}

impl Canned {
    /// A fetcher that returns `body`.
    pub fn serving(body: &str) -> Self {
        Self {
            document: Some(body.to_string()),
            calls: std::cell::Cell::new(0),
        }
    }

    /// A fetcher that cannot reach models.dev.
    pub fn offline() -> Self {
        Self {
            document: None,
            calls: std::cell::Cell::new(0),
        }
    }
}

impl Fetch for Canned {
    async fn document(&self) -> Result<String, QuirksError> {
        self.calls.set(self.calls.get() + 1);
        match &self.document {
            Some(body) => Ok(body.clone()),
            None => Err(QuirksError::Transport("the stub is offline".to_string())),
        }
    }
}
