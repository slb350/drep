//! Asking an endpoint which models it serves.
//!
//! `drep init` used to offer a hardcoded model name per preset and let the user
//! type over it, with nothing checking the result. `presets.rs` said so out
//! loud: the defaults are "the one thing here that goes stale". A typo, or a
//! model the plan does not include, surfaced as a 404 on the first push rather
//! than at the prompt.
//!
//! The endpoint already knows the answer, and the wizard is holding the key at
//! exactly the moment it asks. So it asks the endpoint.
//!
//! ## Why not a registry
//!
//! A vendored catalogue would go stale exactly as the hardcoded defaults do
//! and would additionally have to be noticed and updated. A third-party index
//! (models.dev) covers every provider at once, but describes what a *vendor*
//! publishes rather than what *this account's plan* serves, and it is a 4 MB
//! network dependency on somebody else's uptime. The endpoint is authoritative
//! for the only question being asked.
//!
//! ## One shape, three vendors
//!
//! Every endpoint drep ships a preset for answers `GET {base_url}/models` with
//! `{"data": [{"id": ...}]}`, whichever protocol it otherwise speaks. They
//! disagree only on what *else* is in each entry: z.ai sends OpenAI's
//! `object`/`created`/`owned_by`, MiniMax sends Anthropic's `type`/`created_at`
//! /`display_name`, and Kimi sends both plus `context_length` and
//! `supports_reasoning`. Reading `id` and an optional `display_name` covers all
//! three, and serde ignores the rest - which is also what stops a new field
//! breaking the parse.
//!
//! The protocol still decides the **auth header**, because that is not
//! negotiable per request: bearer for OpenAI-compatible, `x-api-key` plus a
//! version for Anthropic.
//!
//! ## Failure is never fatal
//!
//! A listing is a convenience during setup. An endpoint that does not serve one
//! (a local llama.cpp build, a gateway, anything older) must leave the user
//! typing a name exactly as before, so every error here is something the caller
//! reports and moves past. Nothing in this module can stop `drep init`.

use std::time::Duration;

use open_agent::ApiProtocol;
use serde::Deserialize;
use thiserror::Error;

/// How long to wait for a listing before giving up and letting the user type.
///
/// Short on purpose. This sits between two prompts in an interactive session,
/// and a setup that appears to hang is worse than one that asks for a name.
const TIMEOUT: Duration = Duration::from_secs(10);

/// The API version header Anthropic-shaped endpoints require.
///
/// Duplicated from the SDK rather than imported because the SDK does not export
/// it, and it is a one-line constant whose value is pinned by the same tests
/// that pin the request shape. If it ever needs to change, `list` fails and the
/// wizard falls back to typing a name.
const ANTHROPIC_VERSION: &str = "2023-06-01";

/// A model an endpoint offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Model {
    /// The identifier to put in `drep.toml`.
    pub id: String,
    /// A human-facing name, when the endpoint sends one. Kimi's `k3` is
    /// `"K2.7 Coding"`, which is worth showing beside the id nobody would guess
    /// it from.
    pub display_name: Option<String>,
}

impl Model {
    /// How the wizard lists this model: the id, plus the vendor's own name for
    /// it when that differs.
    pub fn label(&self) -> String {
        match &self.display_name {
            Some(name) if name != &self.id => format!("{} ({name})", self.id),
            _ => self.id.clone(),
        }
    }
}

/// Why a listing could not be produced.
///
/// Every variant is non-fatal: the caller reports it and asks for a name.
#[derive(Debug, Error)]
pub enum ListError {
    #[error("this endpoint does not offer a model list")]
    Unsupported,

    #[error("the endpoint rejected the key (HTTP {0})")]
    Unauthorized(u16),

    #[error("could not reach the endpoint: {0}")]
    Transport(String),

    #[error("the endpoint's model list could not be read: {0}")]
    Malformed(String),
}

/// Where the wizard gets a model list.
///
/// A trait so the wizard can be driven by a stub: the alternative is a wizard
/// test suite that makes real network calls, which would be slow, offline-
/// hostile, and dependent on somebody's plan still including a given model.
pub trait ModelSource {
    /// List the models `endpoint` serves.
    #[allow(async_fn_in_trait)]
    async fn list(
        &self,
        endpoint: &str,
        api_key: &str,
        protocol: ApiProtocol,
    ) -> Result<Vec<Model>, ListError>;
}

/// The largest listing drep will read into memory.
///
/// A real listing is a few kilobytes - the longest of the four is Kimi's, at
/// well under one. 8 MB is a margin no honest endpoint approaches, and it is
/// what stops a mirror, a redirect to something else, or a compromised host
/// making `drep init` allocate without bound. The timeout does not prevent
/// that on its own: a fast host can send a great deal inside one.
const MAX_LISTING_BYTES: u64 = 8 * 1024 * 1024;

/// The real thing: one HTTP GET.
#[derive(Debug, Clone, Copy)]
pub struct Http {
    /// The ceiling on the response body. A field rather than a constant read
    /// directly, so the boundary is reachable from a test without a multi-
    /// megabyte fixture - the same reason [`crate::llm::quirks::Http`] has one.
    max_bytes: u64,
}

impl Http {
    /// A fetcher with the production ceiling.
    pub fn new() -> Self {
        Self {
            max_bytes: MAX_LISTING_BYTES,
        }
    }

    /// The same fetcher with a different size ceiling. For the tests that pin
    /// the boundary; production always uses [`MAX_LISTING_BYTES`].
    #[cfg(test)]
    pub fn with_max_bytes(mut self, max_bytes: u64) -> Self {
        self.max_bytes = max_bytes;
        self
    }
}

impl Default for Http {
    fn default() -> Self {
        Self::new()
    }
}

impl ModelSource for Http {
    async fn list(
        &self,
        endpoint: &str,
        api_key: &str,
        protocol: ApiProtocol,
    ) -> Result<Vec<Model>, ListError> {
        let client = crate::http::client(TIMEOUT).map_err(ListError::Transport)?;

        let request = client.get(url(endpoint));
        let request = match protocol {
            ApiProtocol::Anthropic => request
                .header("x-api-key", api_key)
                .header("anthropic-version", ANTHROPIC_VERSION),
            // `ApiProtocol` is `#[non_exhaustive]`, so a protocol added to the
            // SDK later lands here. Bearer is the right guess for anything
            // OpenAI-shaped, and a wrong one costs a fallback to typing a name.
            _ => request.header("Authorization", format!("Bearer {api_key}")),
        };

        let response = request
            .send()
            .await
            .map_err(|err| ListError::Transport(err.to_string()))?;

        let status = response.status().as_u16();
        if !response.status().is_success() {
            return Err(classify(status));
        }

        // Bounded, not `text()`. This is an endpoint the user typed at a
        // prompt, and drep is holding a key while it asks - so the body has a
        // ceiling for the same reason the registry document does.
        let body = crate::http::read_bounded(response, self.max_bytes)
            .await
            .map_err(|err| match err {
                crate::http::ReadError::Transport(msg) => ListError::Transport(msg),
                crate::http::ReadError::Malformed(msg) => ListError::Malformed(msg),
            })?;
        parse(&body)
    }
}

/// The listing URL for `endpoint`.
///
/// `{base_url}/models` for both protocols - verified against all three
/// subscription endpoints, whose base URLs already carry whatever version
/// segment they use (`/api/coding/paas/v4`, `/anthropic/v1`, `/coding/v1`).
/// A trailing slash on the configured endpoint would otherwise produce `//`,
/// which some gateways answer with a redirect and others with a 404.
fn url(endpoint: &str) -> String {
    format!("{}/models", endpoint.trim_end_matches('/'))
}

/// Map an HTTP status onto the reason the caller reports.
///
/// 404 and 405 are the endpoint saying it has no such route, which is the
/// ordinary case for a local server rather than a fault. 401 and 403 are worth
/// separating because they mean the key is wrong - the user is about to store
/// it, and finding out now beats finding out on the first push.
fn classify(status: u16) -> ListError {
    match status {
        404 | 405 | 501 => ListError::Unsupported,
        401 | 403 => ListError::Unauthorized(status),
        other => ListError::Transport(format!("HTTP {other}")),
    }
}

/// The half of a listing response drep reads.
#[derive(Debug, Deserialize)]
struct Listing {
    data: Vec<Entry>,
}

/// One entry. Every other field the vendors send is ignored by serde.
#[derive(Debug, Deserialize)]
struct Entry {
    id: String,
    #[serde(default)]
    display_name: Option<String>,
}

/// Parse a listing body into models, in the order the endpoint sent them.
///
/// Order is preserved rather than sorted: every one of these endpoints lists
/// its newest model first, which is the one a user setting drep up almost
/// always wants, and alphabetical order would bury it (`MiniMax-M2` sorts above
/// `MiniMax-M3`; `glm-4.5` above `glm-5.3`).
///
/// An empty list is [`ListError::Unsupported`] rather than an empty menu: a
/// prompt offering nothing is worse than the free-text prompt it replaced.
fn parse(body: &str) -> Result<Vec<Model>, ListError> {
    let listing: Listing = serde_json::from_str(body)
        .map_err(|err| ListError::Malformed(crate::text::excerpt(&err.to_string(), 120)))?;

    let models: Vec<Model> = listing
        .data
        .into_iter()
        .filter(|entry| !entry.id.is_empty())
        .map(|entry| Model {
            id: entry.id,
            display_name: entry.display_name,
        })
        .collect();

    if models.is_empty() {
        return Err(ListError::Unsupported);
    }
    Ok(models)
}

#[cfg(test)]
mod tests;
