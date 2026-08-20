//! Backend-neutral failures at the LLM boundary.

use thiserror::Error;

/// What can go wrong at the LLM boundary.
///
/// Every variant means "the file went unanalyzed", but the variants preserve
/// the cause so retry, failover, demotion, and reporting policy never has to
/// infer semantics from a human-readable message.
///
/// `Clone` because the provider chain records the reason a provider went down
/// and hands a copy to every later file that skips it.
#[derive(Debug, Clone, Error)]
pub enum LlmError {
    /// Transport failure after the backend exhausted its retries.
    ///
    /// `status` is the HTTP code when one exists; process failures use `None`.
    #[error("LLM transport failed{}: {message}", status.map(|c| format!(" (HTTP {c})")).unwrap_or_default())]
    Transport {
        status: Option<u16>,
        message: String,
    },

    /// A response arrived but no JSON could be extracted.
    #[error("LLM response was unparseable: {0}")]
    Unparseable(String),

    /// The model stopped before producing JSON, and the server said why.
    ///
    /// It is about the request rather than the provider, so it neither fails
    /// over nor demotes the provider.
    #[error("{message}")]
    ModelStopped { finish: String, message: String },

    /// Configuration is incomplete.
    #[error("LLM not configured: {0}")]
    NotConfigured(String),

    /// A non-HTTP backend classified a failure from structured process state.
    #[error("LLM backend {kind}: {message}")]
    Backend {
        kind: BackendErrorKind,
        message: String,
    },
}

/// Routing class for a structured non-HTTP backend failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendErrorKind {
    Contract,
    Authentication,
    UsageLimit,
    Request,
    UnknownExit,
}

impl std::fmt::Display for BackendErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Contract => "contract failure",
            Self::Authentication => "authentication failure",
            Self::UsageLimit => "usage limit",
            Self::Request => "request rejection",
            Self::UnknownExit => "failure",
        })
    }
}

impl BackendErrorKind {
    /// Stable machine tag used by JSON failure reports.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Contract => "contract",
            Self::Authentication => "authentication",
            Self::UsageLimit => "usage_limit",
            Self::Request => "request",
            Self::UnknownExit => "unknown_exit",
        }
    }
}

impl LlmError {
    /// The HTTP status, when the failure carried one.
    pub fn status(&self) -> Option<u16> {
        match self {
            Self::Transport { status, .. } => *status,
            Self::Unparseable(_)
            | Self::ModelStopped { .. }
            | Self::NotConfigured(_)
            | Self::Backend { .. } => None,
        }
    }
}
