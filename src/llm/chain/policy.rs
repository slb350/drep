//! Provider failover and run-scoped demotion policy.

use crate::llm::error::{BackendErrorKind, LlmError};

/// Whether this failure should be handed to the next provider.
///
/// The whole failover policy lives here so the rule cannot be restated
/// differently at a second site.
pub(super) fn should_failover(err: &LlmError) -> bool {
    match err {
        // No status: a timeout, a refused connection, or an empty body. All
        // provider-level, all worth asking someone else.
        LlmError::Transport { status: None, .. } => true,
        LlmError::Transport {
            status: Some(code), ..
        } => is_retryable_status(*code),
        // A non-empty body we could not parse already exhausted the primary's
        // response retries. A fallback can salvage this file, but the failure
        // remains payload/model-specific and must not demote the provider.
        LlmError::Unparseable(_) => true,
        // A token cap or a content filter is a property of the request. A
        // second provider cannot make the file smaller, and asking one to is
        // the same category error as failing over on a 400. `is_sticky` is
        // defined in terms of this, so it is not remembered either - which
        // matters, because remembering a non-failover failure is what let one
        // bad file stop the chain for every later one.
        LlmError::ModelStopped { .. } => false,
        // Misconfiguration. Routing around it is what hides it.
        LlmError::NotConfigured(_) => false,
        LlmError::Backend { kind, .. } => matches!(kind, BackendErrorKind::UsageLimit),
    }
}

/// Whether this failure is remembered for the rest of the run.
///
/// Deliberately a wider set than [`should_failover`]. A 401 does not advance
/// the chain, but it is still a property of the endpoint rather than of this
/// file: every file in the run will get the same answer, so ask once.
pub(super) fn is_sticky(err: &LlmError) -> bool {
    // Remember a failure only when remembering it cannot change a later file's
    // outcome. Two ways that holds:
    //
    // - The chain advances past this provider anyway, so skipping it costs the
    //   later file nothing it was not already going to pay.
    // - It is a credential the endpoint rejects, which it will reject for every
    //   request regardless of payload.
    //
    // The combination to avoid is a request-dependent failure that is both
    // remembered and non-failing-over: a later file would replay it and stop
    // without contacting anyone. `Contract` is safe despite that shape because
    // it means the process backend violated drep's fixed isolation/event
    // protocol, never that one source payload was rejected. A request-level
    // HTTP 400 is not safe: one oversized payload once poisoned every later
    // file by demoting the provider for the whole run.
    (should_failover(err) && !matches!(err, LlmError::Unparseable(_)))
        || is_auth_failure(err)
        || is_sticky_backend_failure(err)
}

fn is_sticky_backend_failure(err: &LlmError) -> bool {
    matches!(
        err,
        LlmError::Backend {
            kind: BackendErrorKind::Contract | BackendErrorKind::Authentication,
            ..
        }
    )
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
