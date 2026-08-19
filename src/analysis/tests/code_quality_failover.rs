//! The analyzer across a multi-provider chain.
//!
//! `llm/chain/tests` pins the loop itself. What is left to the analyzer is the
//! join: recording who served, mapping a whole-chain failure onto a
//! `FailureReason`, and - the one that ships silently wrong - writing the cache
//! entry under the key of the provider that actually answered.

use wiremock::MockServer;

use super::support::hunks_for_python_at;
use crate::analysis::code_quality::CodeQualityAnalyzer;
use crate::analysis::result::FailureReason;
use crate::llm::chain::ProviderChain;
use crate::test_support::{
    cfg_for, fast_retry_chain, request_count, server_failing_with, server_returning, temp_cache,
};

/// A server answering with one clean finding on `line`.
async fn server_with_finding(line: u32) -> MockServer {
    let body = format!(
        r#"{{"issues": [{{"line": {line}, "severity": "medium", "category": "style", "message": "m"}}], "summary": "s"}}"#
    );
    server_returning(&[&body]).await
}

/// How many files provider `index` answered.
fn served(analyzer: &CodeQualityAnalyzer, index: usize) -> usize {
    analyzer.chain().providers()[index].served()
}

/// The head serving is recorded as provider 0.
#[tokio::test]
async fn a_file_served_by_the_head_is_credited_to_provider_zero() {
    let head = server_with_finding(100).await;
    let (cache, _dir) = temp_cache();
    let analyzer = CodeQualityAnalyzer::new(fast_retry_chain(&[cfg_for(&head, "a", 1)]), cache);

    let result = analyzer.analyze_file(&hunks_for_python_at(100)).await;
    assert_eq!(result.findings.len(), 1);
    assert_eq!(served(&analyzer, 0), 1);
}

/// A file the fallback served is credited to the fallback, not the head.
///
/// Without this, a run that quietly moved every file to a paid endpoint reports
/// exactly what a healthy local run reports.
#[tokio::test]
async fn a_file_served_by_the_fallback_is_credited_to_it() {
    let dead = server_failing_with(500).await;
    let fallback = server_with_finding(100).await;
    let (cache, _dir) = temp_cache();
    let analyzer = CodeQualityAnalyzer::new(
        fast_retry_chain(&[cfg_for(&dead, "a", 1), cfg_for(&fallback, "b", 1)]),
        cache,
    );

    let result = analyzer.analyze_file(&hunks_for_python_at(100)).await;
    assert_eq!(result.findings.len(), 1, "the fallback's findings are kept");
    assert!(result.failed_files.is_empty(), "the file WAS analyzed");
    assert_eq!(served(&analyzer, 1), 1, "the fallback is credited");
    assert_eq!(served(&analyzer, 0), 0, "the head served nothing");
}

/// The cached entry is filed under the model that answered.
///
/// The analyzer-level half of the chain suite's key test, and the one that
/// exercises the actual `cache.put`. A version that put under a key computed
/// from the head would leave the fallback's answer permanently attributed to
/// the local model, and a later healthy run would be served it without ever
/// contacting the local model at all.
#[tokio::test]
async fn the_cache_entry_is_written_under_the_serving_provider_s_key() {
    let dead = server_failing_with(500).await;
    let fallback = server_with_finding(100).await;
    let (cache, _dir) = temp_cache();
    let analyzer = CodeQualityAnalyzer::new(
        fast_retry_chain(&[
            cfg_for(&dead, "model-a", 1),
            cfg_for(&fallback, "model-b", 1),
        ]),
        cache.clone(),
    );

    let hunks = hunks_for_python_at(100);
    let result = analyzer.analyze_file(&hunks).await;
    assert!(result.failed_files.is_empty());

    // The prompt and payload the analyzer used, rebuilt the same way it does,
    // so the keys compared here are the keys production computes.
    let language = crate::languages::detect(&hunks[0].file_path).expect("python");
    let payload = crate::analysis::payload::render(language, &hunks).expect("payload");
    let system = crate::analysis::prompt::build_analysis_prompt(language);

    // Through `Provider::cache_key`, the same call production makes - a key
    // spelled out by hand here would agree with whatever bug production has.
    let key_a = analyzer.chain().providers()[0].cache_key(&cache, &system, &payload.text);
    let key_b = analyzer.chain().providers()[1].cache_key(&cache, &system, &payload.text);
    assert!(
        cache.get(&key_b).is_some(),
        "the fallback's answer must be cached under the fallback's key"
    );
    assert!(
        cache.get(&key_a).is_none(),
        "nothing may be cached under the head's key - it never answered"
    );

    // And the head, restored, is genuinely re-asked rather than served the
    // fallback's cached answer.
    let revived = server_with_finding(100).await;
    let spare = server_with_finding(100).await;
    let second = CodeQualityAnalyzer::new(
        fast_retry_chain(&[
            cfg_for(&revived, "model-a", 1),
            cfg_for(&spare, "model-b", 1),
        ]),
        cache,
    );
    let _ = second.analyze_file(&hunks).await;
    assert_eq!(served(&second, 0), 1, "the restored head serves");
    assert_eq!(
        request_count(&revived).await,
        1,
        "the restored head must actually be contacted"
    );
}

/// Two providers failing produce `AllProviders`, carrying both reasons.
#[tokio::test]
async fn a_chain_where_every_provider_fails_reports_all_of_them() {
    let first = server_failing_with(500).await;
    let second = server_failing_with(503).await;
    let (cache, _dir) = temp_cache();
    let analyzer = CodeQualityAnalyzer::new(
        fast_retry_chain(&[cfg_for(&first, "a", 1), cfg_for(&second, "b", 1)]),
        cache,
    );

    let result = analyzer.analyze_file(&hunks_for_python_at(100)).await;
    let reason = result
        .failed_files
        .values()
        .next()
        .expect("the file is unanalyzed");
    match reason {
        FailureReason::ChainFailed(failures) => {
            assert_eq!(failures.len(), 2);
            assert_eq!(failures[0].model, "a");
            assert_eq!(failures[0].reason.status(), Some(500));
            assert_eq!(failures[1].model, "b");
            assert_eq!(failures[1].reason.status(), Some(503));
        }
        other => panic!("expected ChainFailed, got {other:?}"),
    }
}

/// One provider failing keeps the pre-failover shape.
///
/// The collapse rule: a single-provider config - what `drep init` writes -
/// reports exactly what it reported before failover existed, so neither the
/// text line nor the JSON `kind` changes for the overwhelmingly common case.
#[tokio::test]
async fn a_single_provider_failure_is_not_wrapped_in_all_providers() {
    let only = server_failing_with(500).await;
    let (cache, _dir) = temp_cache();
    let analyzer = CodeQualityAnalyzer::new(fast_retry_chain(&[cfg_for(&only, "a", 1)]), cache);

    let result = analyzer.analyze_file(&hunks_for_python_at(100)).await;
    let reason = result
        .failed_files
        .values()
        .next()
        .expect("the file is unanalyzed");
    assert!(
        matches!(
            reason,
            FailureReason::Transport {
                status: Some(500),
                ..
            }
        ),
        "a lone provider's failure must stay a plain transport failure, got {reason:?}"
    );
}

/// A 401 at the head stops the chain, and the fallback is never asked.
///
/// The end-to-end statement of the rule the chain enforces: routing around a
/// bad credential is how a broken key survives a whole run looking healthy.
#[tokio::test]
async fn a_401_at_the_head_does_not_reach_the_fallback() {
    let unauthorized = server_failing_with(401).await;
    let fallback = server_with_finding(100).await;
    let (cache, _dir) = temp_cache();
    let analyzer = CodeQualityAnalyzer::new(
        fast_retry_chain(&[cfg_for(&unauthorized, "a", 1), cfg_for(&fallback, "b", 1)]),
        cache,
    );

    let result = analyzer.analyze_file(&hunks_for_python_at(100)).await;
    assert!(!result.failed_files.is_empty(), "the file is unanalyzed");
    assert_eq!(request_count(&fallback).await, 0);
    assert_eq!(served(&analyzer, 0), 0, "nobody served the file");
    assert_eq!(served(&analyzer, 1), 0);
}

/// Across files, the per-provider counts sum.
#[tokio::test]
async fn provider_counts_sum_across_files() {
    let dead = server_failing_with(500).await;
    let fallback = server_with_finding(100).await;
    let (cache, _dir) = temp_cache();
    // One permit on the head, which makes the demotion deterministic rather
    // than a race: file one takes the slot and file two waits behind it, so by
    // the time file two is admitted the head is already marked down and the
    // post-acquire re-check skips it. With the default three permits both
    // files would be in flight before the first failure landed, and the head
    // would legitimately see two requests.
    let mut head = cfg_for(&dead, "a", 1);
    head.max_concurrent = 1;
    let analyzer =
        CodeQualityAnalyzer::new(fast_retry_chain(&[head, cfg_for(&fallback, "b", 1)]), cache);

    let by_file = vec![
        vec![super::support::python_hunk("src/one.py", 100)],
        vec![super::support::python_hunk("src/two.py", 100)],
    ];
    let _ = analyzer.analyze_files(&by_file).await;
    assert_eq!(served(&analyzer, 1), 2, "two files served by the fallback");
    assert_eq!(
        request_count(&dead).await,
        1,
        "sticky demotion: the dead head is contacted once, not once per file"
    );
}

/// A chain built from a `Config` uses only the enabled entries.
///
/// The join between `config::providers()` and the chain. A disabled head must
/// not be built into the chain at all - `ProviderChain::new` rejects a disabled
/// entry outright, so a caller that forgot the filter fails loudly rather than
/// producing a chain whose numbering does not match the config file.
#[test]
fn a_chain_is_built_from_the_enabled_providers_only() {
    let disabled = crate::config::LlmConfig {
        enabled: false,
        endpoint: Some("http://parked/v1".to_owned()),
        model: Some("parked".to_owned()),
        ..crate::config::LlmConfig::default()
    };
    let live = crate::config::LlmConfig {
        endpoint: Some("http://live/v1".to_owned()),
        model: Some("live".to_owned()),
        ..crate::config::LlmConfig::default()
    };
    let config = crate::config::Config {
        llm: vec![disabled, live],
    };

    let chain = ProviderChain::new(&config.providers()).expect("chain builds");
    assert_eq!(chain.providers().len(), 1);
    assert_eq!(chain.providers()[0].model(), "live");
}
