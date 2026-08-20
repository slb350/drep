//! HTTP and Codex are distinct implementations under one provider contract.

use std::ffi::OsString;

use crate::config::{BackendKind, LlmConfig, ReasoningEffort};
use crate::llm::backend::ProviderBackend;
use crate::llm::chain::{Provider, ProviderChain};
use crate::llm::codex::CodexClient;
use crate::test_support::{cfg_for, temp_cache};

use super::support::{CONTENT, SYSTEM, server_returning_json};

#[tokio::test]
async fn openai_api_still_posts_the_same_chat_completion_request() {
    let server = server_returning_json().await;
    let mut cfg = cfg_for(&server, "gpt-5.6-sol", 1);
    cfg.api_key = Some("openai-test-key".to_owned());
    cfg.temperature = None;
    cfg.max_tokens = None;
    let chain = ProviderChain::new(&[&cfg]).expect("HTTP chain");
    let (cache, _dir) = temp_cache();

    chain
        .complete_json(SYSTEM, CONTENT, &cache)
        .await
        .expect("mock OpenAI response");

    let requests = server.received_requests().await.expect("requests");
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    assert_eq!(request.url.path(), "/v1/chat/completions");
    assert_eq!(
        request
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok()),
        Some("Bearer openai-test-key")
    );
    let body: serde_json::Value = serde_json::from_slice(&request.body).expect("JSON body");
    assert_eq!(body["model"], "gpt-5.6-sol");
    assert!(body.get("temperature").is_none(), "got {body}");
    assert!(body.get("max_tokens").is_none(), "got {body}");
}

#[test]
fn backend_identity_and_cache_keys_cannot_collide_for_the_same_model() {
    let http_cfg = LlmConfig {
        endpoint: Some("https://api.openai.com/v1".to_owned()),
        model: Some("gpt-5.6-sol".to_owned()),
        ..LlmConfig::default()
    };
    let codex_cfg = LlmConfig {
        backend: BackendKind::Codex,
        model: Some("gpt-5.6-sol".to_owned()),
        reasoning_effort: Some(ReasoningEffort::High),
        max_concurrent: 1,
        ..LlmConfig::default()
    };
    let http = ProviderBackend::Http(crate::llm::client::LlmClient::new(&http_cfg).unwrap());
    let codex = ProviderBackend::Codex(
        CodexClient::for_test(
            &codex_cfg,
            "unused-codex",
            [(OsString::from("PATH"), OsString::from("/bin"))],
            "0.148.0",
        )
        .unwrap(),
    );
    assert_ne!(http.identity(), codex.identity());

    let (cache, _dir) = temp_cache();
    let http = Provider::for_test(http, 1);
    let codex = Provider::for_test(codex, 1);
    assert_ne!(
        http.cache_key(&cache, SYSTEM, CONTENT),
        codex.cache_key(&cache, SYSTEM, CONTENT)
    );
}

#[tokio::test]
async fn same_model_http_to_codex_failover_returns_the_codex_cache_key() {
    let dead = crate::test_support::server_failing_with(500).await;
    let model = "gpt-5.6-sol";
    let http = ProviderBackend::Http(
        crate::llm::client::LlmClient::new(&cfg_for(&dead, model, 1)).unwrap(),
    );

    let dir = tempfile::tempdir().expect("tempdir");
    let executable = dir.path().join("fake-codex");
    crate::test_support::write_executable(
        &executable,
        concat!(
            "#!/bin/sh\n",
            "sed -n '1,$p' >/dev/null\n",
            "printf '%s\\n' \\\n",
            "  '{\"type\":\"item.completed\",\"item\":{\"type\":\"agent_message\",\"text\":\"{\\\"issues\\\":[],\\\"summary\\\":\\\"clean\\\"}\"}}' \\\n",
            "  '{\"type\":\"turn.completed\"}'\n",
        ),
    );
    let codex_cfg = LlmConfig {
        backend: BackendKind::Codex,
        model: Some(model.to_owned()),
        reasoning_effort: Some(ReasoningEffort::High),
        timeout_secs: 5,
        max_concurrent: 1,
        ..LlmConfig::default()
    };
    let codex = ProviderBackend::Codex(
        CodexClient::for_test(
            &codex_cfg,
            executable,
            [
                (OsString::from("PATH"), OsString::from("/usr/bin:/bin")),
                (OsString::from("HOME"), OsString::from("/safe/home")),
            ],
            "0.148.0",
        )
        .unwrap(),
    );
    let chain = ProviderChain::for_test([(http, 1), (codex, 1)]);
    let (cache, _dir) = temp_cache();

    let served = chain
        .complete_json(SYSTEM, CONTENT, &cache)
        .await
        .expect("Codex fallback answers");

    assert_eq!(served.provider, 1);
    assert_eq!(
        served.key,
        chain.providers()[1].cache_key(&cache, SYSTEM, CONTENT)
    );
    assert_ne!(
        served.key,
        chain.providers()[0].cache_key(&cache, SYSTEM, CONTENT)
    );
}
