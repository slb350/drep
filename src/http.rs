//! The two pieces of HTTP drep performs for itself.
//!
//! Reviews go through open-agent-sdk, which owns its own transport. What is
//! left is `drep init` asking two questions over plain GET: which models an
//! endpoint serves ([`crate::llm::models`]) and what those models accept
//! ([`crate::llm::quirks`]). Both are one request against a host the user
//! named, both must be bounded, must never follow a redirect, and must be
//! non-fatal.
//!
//! They live here rather than in either module because the bound is a safety
//! property, and a safety property written twice is written once and forgotten
//! once. That is exactly what happened: the quirks fetcher was given a size
//! ceiling and a chunked read, while the older listing fetcher next to it kept
//! calling `text()` with no ceiling at all - against an endpoint typed at a
//! prompt, while holding a key.
//!
//! What is deliberately *not* shared is classification. The two callers
//! disagree about what a status means - a 404 is an ordinary answer for a
//! listing and a fault for the registry - so each keeps its own error enum and
//! maps [`ReadError`] into it.

use std::time::Duration;

use thiserror::Error;

/// Why a body could not be read.
///
/// Split the way both callers already split their own errors: something went
/// wrong with the transfer, or the bytes are not text.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ReadError {
    #[error("{0}")]
    Transport(String),

    #[error("{0}")]
    Malformed(String),
}

/// A client with `timeout` covering the whole request and redirects disabled.
///
/// The one place a proxy, a user agent or a redirect policy would ever go.
/// There used to be two of these, differing by accident rather than by choice.
/// Neither caller follows redirects: the configured URL is the exact origin
/// that may receive its request, and the model listing carries a credential.
///
/// The error is a `String` so each caller can map it into its own enum without
/// this module knowing about either.
pub fn client(timeout: Duration) -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(timeout)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|err| err.to_string())
}

/// Read a response body, refusing one larger than `max_bytes`.
///
/// A declared `Content-Length` past the ceiling is refused before a byte is
/// read, but that is a shortcut rather than the guarantee: reqwest strips the
/// header from a response it decompresses, and chunked transfer encoding never
/// sends one. The per-chunk cap is what actually holds, and it counts *decoded*
/// bytes - which is both what gets allocated and what makes a body that
/// inflates without limit refusable.
///
/// The timeout on the client is not a size bound. A fast host can send a great
/// deal inside one, and the body is buffered whole.
pub async fn read_bounded(
    response: reqwest::Response,
    max_bytes: u64,
) -> Result<String, ReadError> {
    if let Some(len) = response.content_length()
        && len > max_bytes
    {
        return Err(ReadError::Transport(format!(
            "the response declares {len} bytes, past the {max_bytes}-byte limit"
        )));
    }

    let mut body = Vec::new();
    let mut stream = response;
    while let Some(chunk) = stream
        .chunk()
        .await
        .map_err(|err| ReadError::Transport(err.to_string()))?
    {
        body.extend_from_slice(&chunk);
        if body.len() as u64 > max_bytes {
            return Err(ReadError::Transport(format!(
                "the response exceeded the {max_bytes}-byte limit"
            )));
        }
    }

    String::from_utf8(body)
        .map_err(|err| ReadError::Malformed(crate::text::excerpt(&err.to_string(), 120)))
}

#[cfg(test)]
mod tests;
