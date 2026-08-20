//! Backend failures route by typed kind, never by matching message prose.

use crate::llm::error::{BackendErrorKind, LlmError};

use super::super::{is_sticky, should_failover};

fn backend(kind: BackendErrorKind, message: &str) -> LlmError {
    LlmError::Backend {
        kind,
        message: message.to_owned(),
    }
}

#[test]
fn backend_error_policy_is_driven_only_by_the_kind() {
    for message in ["quota", "same words but misleading: unauthorized timeout"] {
        let contract = backend(BackendErrorKind::Contract, message);
        assert!(!should_failover(&contract));
        assert!(is_sticky(&contract));

        let auth = backend(BackendErrorKind::Authentication, message);
        assert!(!should_failover(&auth));
        assert!(is_sticky(&auth));

        let usage = backend(BackendErrorKind::UsageLimit, message);
        assert!(should_failover(&usage));
        assert!(is_sticky(&usage));

        let request = backend(BackendErrorKind::Request, message);
        assert!(!should_failover(&request));
        assert!(!is_sticky(&request));

        let unknown = backend(BackendErrorKind::UnknownExit, message);
        assert!(!should_failover(&unknown));
        assert!(!is_sticky(&unknown));
    }
}
