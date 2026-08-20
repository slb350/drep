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

/// A models.dev-shaped document, cut down to the cases that matter.
///
/// Field-for-field the shape of the real one, verified against
/// `https://models.dev/api.json`: providers keyed by vendor id, each with an
/// `api` URL that may be null, and models keyed by id carrying `temperature`
/// and `limit.output` among many fields drep ignores.
pub(crate) const DOCUMENT: &str = r#"{
  "kimi-for-coding": {
    "id": "kimi-for-coding",
    "name": "Kimi For Coding",
    "api": "https://api.kimi.com/coding/v1",
    "models": {
      "k3": {
        "id": "k3",
        "name": "Kimi K3",
        "reasoning": true,
        "temperature": false,
        "limit": { "context": 262144, "output": 131072 }
      },
      "kimi-for-coding": {
        "id": "kimi-for-coding",
        "temperature": false,
        "limit": { "context": 262144, "output": 32768 }
      }
    }
  },
  "zai-coding-plan": {
    "id": "zai-coding-plan",
    "api": "https://api.z.ai/api/coding/paas/v4",
    "models": {
      "glm-5.3": {
        "id": "glm-5.3",
        "temperature": true,
        "limit": { "context": 204800, "output": 131072 }
      }
    }
  },
  "openai": {
    "id": "openai",
    "api": null,
    "models": {
      "gpt-5.6-sol": {
        "id": "gpt-5.6-sol",
        "temperature": false,
        "limit": { "context": 400000, "output": 128000 }
      }
    }
  },
  "blank-endpoint": {
    "id": "blank-endpoint",
    "api": "   ",
    "models": { "nowhere": { "id": "nowhere", "temperature": false } }
  },
  "quiet-vendor": {
    "id": "quiet-vendor",
    "api": "https://quiet.example/v1",
    "models": { "unspecified": { "id": "unspecified" } }
  }
}"#;

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
