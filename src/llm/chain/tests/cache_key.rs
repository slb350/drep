//! The cache key moves with the provider.
//!
//! This is the file the phase turns on. A test that mounts a dead provider A
//! and a healthy provider B proves the loop advances; it proves nothing about
//! whether the key advanced with it. If it did not, provider B's answer is
//! filed under provider A's key, and the next run - with A back up - gets a
//! hit that never came from A. That bug ships green under every test in
//! `failover.rs`.

use serde_json::json;

use super::support::{CONTENT, GOOD_JSON, SYSTEM, server_returning_json};
use crate::llm::json_parsing::Extracted;
use crate::test_support::{
    cfg_for, fast_retry_chain, request_count, server_failing_with, temp_cache,
};

/// The key handed back names the model that answered, not the model that was
/// asked first.
#[tokio::test]
async fn the_returned_key_belongs_to_the_provider_that_answered() {
    let dead = server_failing_with(500).await;
    let healthy = server_returning_json().await;
    let (cache, _dir) = temp_cache();
    let head = cfg_for(&dead, "model-a", 1);
    let tail = cfg_for(&healthy, "model-b", 1);
    let chain = fast_retry_chain(&[head, tail]);

    let served = chain
        .complete_json(SYSTEM, CONTENT, &cache)
        .await
        .expect("provider 2 answers");

    let key_a = chain.providers()[0].cache_key(&cache, SYSTEM, CONTENT);
    let key_b = chain.providers()[1].cache_key(&cache, SYSTEM, CONTENT);
    assert_ne!(key_a, key_b, "the two models must key differently");
    assert_eq!(
        served.key, key_b,
        "the answer must be filed under the key of the provider that gave it"
    );
    assert_ne!(
        served.key, key_a,
        "filing provider 2's answer under provider 1's key is the bug this test exists for"
    );
}

/// The full round trip: B's answer is stored, then A comes back up and is
/// asked again rather than served B's cached answer.
///
/// The assertion that matters is `request_count(&revived) == 1`. A chain that
/// keyed on the head would find a hit here and never contact A at all - and
/// the user would be told their local model reviewed the file when a paid
/// endpoint had.
#[tokio::test]
async fn a_later_run_with_the_head_restored_does_not_get_the_fallback_s_cached_answer() {
    let (cache, _dir) = temp_cache();

    // Run one: the head is down, the tail answers, the caller stores it.
    {
        let dead = server_failing_with(500).await;
        let healthy = server_returning_json().await;
        let chain = fast_retry_chain(&[
            cfg_for(&dead, "model-a", 1),
            cfg_for(&healthy, "model-b", 1),
        ]);
        let served = chain
            .complete_json(SYSTEM, CONTENT, &cache)
            .await
            .expect("provider 2 answers");
        assert_eq!(served.provider, 1);
        cache
            .put(&served.key, &json!({"issues": [], "summary": "clean"}))
            .expect("cache write");
    }

    // Run two: a fresh chain, the head restored. It must be asked.
    let revived = server_returning_json().await;
    let spare = server_returning_json().await;
    let chain = fast_retry_chain(&[
        cfg_for(&revived, "model-a", 1),
        cfg_for(&spare, "model-b", 1),
    ]);
    let served = chain
        .complete_json(SYSTEM, CONTENT, &cache)
        .await
        .expect("the restored head answers");

    assert_eq!(served.provider, 0, "the restored head serves the file");
    assert!(
        !served.from_cache,
        "the head has no cached answer of its own - the stored entry was the fallback's"
    );
    assert_eq!(
        request_count(&revived).await,
        1,
        "the restored head must actually be contacted"
    );
}

/// A cached answer for the head short-circuits the whole chain, and costs no
/// request anywhere.
#[tokio::test]
async fn a_cache_hit_on_the_head_asks_nobody() {
    let head = server_returning_json().await;
    let tail = server_returning_json().await;
    let (cache, _dir) = temp_cache();
    let chain = fast_retry_chain(&[cfg_for(&head, "model-a", 1), cfg_for(&tail, "model-b", 1)]);

    let planted = serde_json::from_str::<serde_json::Value>(GOOD_JSON).expect("fixture parses");
    cache
        .put(
            &chain.providers()[0].cache_key(&cache, SYSTEM, CONTENT),
            &planted,
        )
        .expect("cache write");

    let served = chain
        .complete_json(SYSTEM, CONTENT, &cache)
        .await
        .expect("the cache answers");

    assert_eq!(served.provider, 0);
    assert!(served.from_cache);
    assert!(matches!(served.extracted, Extracted::Complete(_)));
    assert_eq!(request_count(&head).await, 0);
    assert_eq!(request_count(&tail).await, 0);
}

/// A cached answer for the *fallback* is used when the head is down, without
/// contacting the fallback.
///
/// The discriminating half of the previous test: a chain that computed the key
/// once, before the loop, would look up the head's key against the fallback and
/// miss - paying for a request it did not need.
#[tokio::test]
async fn a_cache_hit_on_the_fallback_is_found_after_the_head_fails() {
    let dead = server_failing_with(500).await;
    let tail = server_returning_json().await;
    let (cache, _dir) = temp_cache();
    let chain = fast_retry_chain(&[cfg_for(&dead, "model-a", 1), cfg_for(&tail, "model-b", 1)]);

    let planted = serde_json::from_str::<serde_json::Value>(GOOD_JSON).expect("fixture parses");
    cache
        .put(
            &chain.providers()[1].cache_key(&cache, SYSTEM, CONTENT),
            &planted,
        )
        .expect("cache write");

    let served = chain
        .complete_json(SYSTEM, CONTENT, &cache)
        .await
        .expect("the fallback's cache answers");

    assert_eq!(served.provider, 1);
    assert!(served.from_cache);
    assert_eq!(
        request_count(&tail).await,
        0,
        "the fallback's cached answer must be found before a request is made"
    );
}

/// Two providers running the *same model* at different endpoints do not share
/// a cache entry.
///
/// The sibling tests above all use distinct model names, which is what let this
/// through: the key was built from the model and the temperature alone, so a
/// local llama.cpp and a cloud endpoint both serving `qwen3-30b-a3b` - the
/// canonical failover pair, one open model in two places - produced the *same*
/// key. The fallback's answer was then filed where the head would look for its
/// own, and a later run with the head restored got a hit it never produced.
/// Same defect as keying on the head; invisible to a test that varies the
/// model.
#[tokio::test]
async fn two_endpoints_serving_the_same_model_do_not_share_a_cache_entry() {
    let (cache, _dir) = temp_cache();
    const MODEL: &str = "qwen3-30b-a3b";

    // Run one's servers are bound here, not inside the block, and stay alive
    // for the whole test. Dropping them frees their ephemeral ports, and Linux
    // hands the next listener a port it just released - so `revived` below came
    // up on the address `healthy` had used, the two endpoints were the same
    // string, and the keys matched. That failed only on Linux, and only in CI:
    // macOS does not recycle a port that eagerly.
    let dead = server_failing_with(500).await;
    let healthy = server_returning_json().await;

    // Run one: the head is down, the fallback answers, the caller stores it.
    let fallback_key = {
        let chain = fast_retry_chain(&[cfg_for(&dead, MODEL, 1), cfg_for(&healthy, MODEL, 1)]);
        let served = chain
            .complete_json(SYSTEM, CONTENT, &cache)
            .await
            .expect("the fallback answers");
        assert_eq!(served.provider, 1);
        cache
            .put(&served.key, &json!({"issues": [], "summary": "clean"}))
            .expect("cache write");
        served.key
    };

    // Run two: the head is back. It shares the model name, so a key built from
    // the model alone would hit the fallback's entry.
    let revived = server_returning_json().await;
    let spare = server_returning_json().await;
    assert_ne!(
        revived.uri(),
        healthy.uri(),
        "the test needs two genuinely different endpoints to compare keys"
    );
    let chain = fast_retry_chain(&[cfg_for(&revived, MODEL, 1), cfg_for(&spare, MODEL, 1)]);
    let served = chain
        .complete_json(SYSTEM, CONTENT, &cache)
        .await
        .expect("the restored head answers");

    assert_eq!(served.provider, 0, "the restored head serves the file");
    assert_ne!(
        served.key, fallback_key,
        "two endpoints must key differently even when they name the same model"
    );
    assert!(
        !served.from_cache,
        "the head has no entry of its own - the stored one was the fallback's"
    );
    assert_eq!(
        request_count(&revived).await,
        1,
        "the restored head must actually be contacted"
    );
}

/// Two providers differing only in `temperature` do not share a cache entry.
///
/// `Provider::cache_key` reads the temperature through `LlmClient::temperature`,
/// while the request itself reads the field directly - so an accessor returning
/// a constant would send the configured temperature and file the answer under a
/// key naming a different one. That is the same shape as the missing endpoint:
/// the request goes one place and the key names another, and every
/// varies-the-model test passes throughout.
#[tokio::test]
async fn two_providers_differing_only_in_temperature_key_differently() {
    let server = server_returning_json().await;
    let (cache, _dir) = temp_cache();

    let mut cool = cfg_for(&server, "same-model", 1);
    cool.temperature = Some(0.0);
    let mut warm = cfg_for(&server, "same-model", 1);
    warm.temperature = Some(1.0);

    let chain = fast_retry_chain(&[cool, warm]);
    let key_cool = chain.providers()[0].cache_key(&cache, SYSTEM, CONTENT);
    let key_warm = chain.providers()[1].cache_key(&cache, SYSTEM, CONTENT);

    assert_ne!(
        key_cool, key_warm,
        "temperature changes the answer, so it must change the key"
    );
}

/// Same endpoint, same model, two protocols. `api.minimax.io` genuinely serves
/// `MiniMax-M3` over both `/v1` and `/anthropic/v1`, so this is the shape the
/// key has to separate - and neither the endpoint nor the model does it.
#[tokio::test]
async fn two_providers_differing_only_in_protocol_key_differently() {
    let server = server_returning_json().await;
    let (cache, _dir) = temp_cache();

    let openai = cfg_for(&server, "same-model", 1);
    let mut anthropic = cfg_for(&server, "same-model", 1);
    anthropic.protocol = Some("anthropic".into());

    let chain = fast_retry_chain(&[openai, anthropic]);
    let key_openai = chain.providers()[0].cache_key(&cache, SYSTEM, CONTENT);
    let key_anthropic = chain.providers()[1].cache_key(&cache, SYSTEM, CONTENT);

    assert_ne!(
        key_openai, key_anthropic,
        "two protocols are two requests, so they must be two keys"
    );
}

/// An unset temperature is a different request from any set one, and
/// `Provider::cache_key` is the single definition that has to say so.
#[tokio::test]
async fn an_unset_temperature_keys_differently_through_the_provider() {
    let server = server_returning_json().await;
    let (cache, _dir) = temp_cache();

    let mut unset = cfg_for(&server, "same-model", 1);
    unset.temperature = None;
    let mut set = cfg_for(&server, "same-model", 1);
    set.temperature = Some(0.2);

    let chain = fast_retry_chain(&[unset, set]);

    assert_ne!(
        chain.providers()[0].cache_key(&cache, SYSTEM, CONTENT),
        chain.providers()[1].cache_key(&cache, SYSTEM, CONTENT),
        "omitting the parameter lets the server pick, so the answers differ"
    );
}
