//! `ProviderChain::new` - what it accepts, and what it refuses.

use crate::config::LlmConfig;
use crate::llm::chain::ProviderChain;
use crate::llm::client::LlmError;

/// An empty chain is refused rather than tolerated as "nothing to do".
///
/// `config::load` already rejects a config with no enabled provider, so this
/// is unreachable through the CLI - but `ProviderChain` is constructible
/// directly, and a chain of zero providers would report every file as failing
/// with an empty attempt list, which reads like a bug in drep rather than a
/// broken config.
#[test]
fn an_empty_chain_is_refused() {
    let err = ProviderChain::new(&[]).expect_err("a chain needs at least one provider");
    match err {
        LlmError::NotConfigured(message) => assert!(
            message.contains("[[llm]]"),
            "the message must name the config section, got {message:?}"
        ),
        other => panic!("expected NotConfigured, got {other:?}"),
    }
}

/// A misconfigured entry is fatal, and the message says *which* entry.
///
/// Skipping it would be the same masking the 401 rule forbids. The index is
/// what makes the message actionable: with three providers, "LLM endpoint is
/// not set in config" does not say which block to fix. One-based, matching how
/// `doctor` numbers the same list.
#[test]
fn a_misconfigured_entry_is_fatal_and_names_its_position() {
    let good = LlmConfig {
        endpoint: Some("http://localhost:1/v1".to_owned()),
        model: Some("a".to_owned()),
        ..LlmConfig::default()
    };
    let no_endpoint = LlmConfig {
        model: Some("b".to_owned()),
        ..LlmConfig::default()
    };

    let err = ProviderChain::new(&[&good, &no_endpoint]).expect_err("entry 2 has no endpoint");
    match err {
        LlmError::NotConfigured(message) => {
            assert!(
                message.contains("#2"),
                "the message must name the second entry, got {message:?}"
            );
            assert!(
                message.contains("endpoint"),
                "the message must keep the underlying reason, got {message:?}"
            );
        }
        other => panic!("expected NotConfigured, got {other:?}"),
    }
}

/// A disabled entry reaching the chain is a caller bug, not something to skip.
///
/// `Config::providers()` is what filters on `enabled`; the chain is handed the
/// survivors. If a disabled config reaches here, the filter was bypassed - so
/// this fails loudly rather than silently shortening the chain and producing a
/// run whose provider numbering does not match the config file.
#[test]
fn a_disabled_entry_reaching_the_chain_is_an_error() {
    let disabled = LlmConfig {
        enabled: false,
        endpoint: Some("http://localhost:1/v1".to_owned()),
        model: Some("a".to_owned()),
        ..LlmConfig::default()
    };
    let err = ProviderChain::new(&[&disabled]).expect_err("a disabled entry must not build");
    assert!(
        matches!(err, LlmError::NotConfigured(_)),
        "expected NotConfigured, got {err:?}"
    );
}

/// Providers keep their order, and each one reports its own model and
/// endpoint.
///
/// Order is the whole contract: the list is a preference order, and a chain
/// that reordered it would fail over to the wrong provider while every
/// "did it advance" test still passed.
#[test]
fn providers_keep_their_order_with_their_own_model_and_endpoint() {
    let first = LlmConfig {
        endpoint: Some("http://first/v1".to_owned()),
        model: Some("model-one".to_owned()),
        ..LlmConfig::default()
    };
    let second = LlmConfig {
        endpoint: Some("http://second/v1".to_owned()),
        model: Some("model-two".to_owned()),
        ..LlmConfig::default()
    };

    let chain = ProviderChain::new(&[&first, &second]).expect("chain builds");
    let seen: Vec<(&str, &str)> = chain
        .providers()
        .iter()
        .map(|p| (p.model(), p.endpoint()))
        .collect();
    assert_eq!(
        seen,
        vec![
            ("model-one", "http://first/v1"),
            ("model-two", "http://second/v1"),
        ]
    );
}

/// A freshly built chain has demoted nobody.
#[test]
fn a_fresh_chain_has_no_provider_down() {
    let cfg = LlmConfig {
        endpoint: Some("http://localhost:1/v1".to_owned()),
        model: Some("a".to_owned()),
        ..LlmConfig::default()
    };
    let chain = ProviderChain::new(&[&cfg]).expect("chain builds");
    assert!(!chain.providers()[0].is_down());
    assert_eq!(chain.providers()[0].served(), 0);
}
