//! Narrowing a preset's values against what the registry knows.

use super::super::{Quirks, Registry, resolve};
use super::DOCUMENT;

const KIMI: &str = "https://api.kimi.com/coding/v1";
const ZAI: &str = "https://api.z.ai/api/coding/paas/v4";

fn registry() -> Registry {
    Registry::distil(DOCUMENT, 0).expect("the fixture document distils")
}

/// The `kimi` preset's own values: no temperature, a required `max_tokens`.
fn kimi_defaults() -> Quirks {
    Quirks {
        temperature: None,
        max_tokens: Some(200_000),
        max_tokens_from_registry: false,
    }
}

/// The `zai` preset's own values: a temperature, no `max_tokens`.
fn zai_defaults() -> Quirks {
    Quirks {
        temperature: Some(0.2),
        max_tokens: None,
        max_tokens_from_registry: false,
    }
}

#[test]
fn no_registry_at_all_keeps_the_presets_values() {
    // The offline case, and the `--provider` flag path, which never fetches.
    assert_eq!(resolve(None, kimi_defaults(), KIMI, "k3"), kimi_defaults());
    assert_eq!(
        resolve(None, zai_defaults(), ZAI, "glm-5.3"),
        zai_defaults()
    );
}

#[test]
fn a_model_the_registry_does_not_name_keeps_the_presets_values() {
    // Released after the cache was written, or typed at the free-text prompt.
    assert_eq!(
        resolve(Some(&registry()), kimi_defaults(), KIMI, "k4-preview"),
        kimi_defaults()
    );
}

#[test]
fn an_endpoint_the_registry_does_not_name_keeps_the_presets_values() {
    // A local server, a gateway, a `custom` endpoint models.dev never heard of.
    assert_eq!(
        resolve(
            Some(&registry()),
            zai_defaults(),
            "http://localhost:1234/v1",
            "glm-5.3"
        ),
        zai_defaults(),
        "the model id alone must not match another provider's entry"
    );
}

#[test]
fn a_required_max_tokens_becomes_the_models_own_limit() {
    // The point of the whole exercise. `k3` publishes 131,072, and the preset's
    // 200,000 is a number nobody checked against the model.
    let resolved = resolve(Some(&registry()), kimi_defaults(), KIMI, "k3");

    assert_eq!(resolved.max_tokens, Some(131_072));
    assert!(resolved.max_tokens_from_registry);
}

#[test]
fn a_second_model_on_the_same_endpoint_gets_its_own_limit() {
    // The provider-scoped guess is wrong by a factor of six here, which is what
    // "it is a property of the model" means in practice.
    let resolved = resolve(Some(&registry()), kimi_defaults(), KIMI, "kimi-for-coding");

    assert_eq!(resolved.max_tokens, Some(32_768));
}

#[test]
fn a_named_model_with_no_published_limit_keeps_the_presets_fallback() {
    // The endpoint still requires the field, so something has to be written.
    let mut defaults = kimi_defaults();
    defaults.max_tokens = Some(200_000);

    let resolved = resolve(
        Some(&registry()),
        defaults,
        "https://quiet.example/v1",
        "unspecified",
    );

    assert_eq!(resolved.max_tokens, Some(200_000));
    assert!(
        !resolved.max_tokens_from_registry,
        "the rendered comment must not claim this is the model's own limit"
    );
}

#[test]
fn the_registry_never_introduces_a_max_tokens() {
    // `glm-5.3` publishes an output limit, and z.ai does not require the field.
    // Writing one would put a completion cap on a reasoning model, which is the
    // coupling 2.0 removed.
    let resolved = resolve(Some(&registry()), zai_defaults(), ZAI, "glm-5.3");

    assert_eq!(resolved.max_tokens, None);
    assert!(!resolved.max_tokens_from_registry);
}

#[test]
fn a_model_that_refuses_temperature_loses_it() {
    // Withdrawing is the safe direction: a parameter drep omits costs default
    // sampling, and one the model rejects is a 400 that neither fails over nor
    // retries.
    let mut defaults = kimi_defaults();
    defaults.temperature = Some(0.2);

    let resolved = resolve(Some(&registry()), defaults, KIMI, "k3");

    assert_eq!(resolved.temperature, None);
}

#[test]
fn a_model_that_accepts_temperature_keeps_the_presets_value() {
    let resolved = resolve(Some(&registry()), zai_defaults(), ZAI, "glm-5.3");

    assert_eq!(resolved.temperature, Some(0.2));
}

#[test]
fn the_registry_never_introduces_a_temperature() {
    // `zai_defaults` with the temperature taken away stands in for a preset
    // that deliberately sends none. The registry saying the model would accept
    // one is not a reason to start sending it: that is the direction that
    // produces the 400 this feature exists to prevent.
    let mut defaults = zai_defaults();
    defaults.temperature = None;

    let resolved = resolve(Some(&registry()), defaults, ZAI, "glm-5.3");

    assert_eq!(resolved.temperature, None);
}

/// A registry with one endpoint serving one model, for the boundary cases the
/// shared fixture document does not contain.
fn one_model(endpoint: &str, model: &str, temperature: bool, output: Option<u32>) -> Registry {
    let limit = match output {
        Some(value) => format!(r#", "limit": {{ "output": {value} }}"#),
        None => String::new(),
    };
    let document = format!(
        r#"{{"p": {{"api": "{endpoint}", "models": {{"{model}": {{"id": "{model}",
           "temperature": {temperature}{limit} }} }} }} }}"#
    );
    Registry::distil(&document, 0).expect("distils")
}

#[test]
fn a_published_limit_above_the_presets_fallback_never_raises_it() {
    // The preset's value is one drep has verified the endpoint accepts. A
    // published limit above it is a claim drep has not tested, and raising a
    // required ceiling is the direction that produces a 400 - which by
    // invariant neither fails over nor retries.
    let registry = one_model(KIMI, "k3", false, Some(999_999));

    let resolved = resolve(Some(&registry), kimi_defaults(), KIMI, "k3");

    assert_eq!(resolved.max_tokens, Some(200_000), "narrowed, never raised");
    assert!(
        !resolved.max_tokens_from_registry,
        "and the rendered comment must not credit the registry for it"
    );
}

#[test]
fn a_published_limit_below_the_fallback_is_taken() {
    // The case the registry exists for.
    let registry = one_model(KIMI, "kimi-for-coding", false, Some(32_768));

    let resolved = resolve(Some(&registry), kimi_defaults(), KIMI, "kimi-for-coding");

    assert_eq!(resolved.max_tokens, Some(32_768));
    assert!(resolved.max_tokens_from_registry);
}

#[test]
fn two_providers_sharing_an_endpoint_contribute_both_model_lists() {
    // models.dev publishes `minimax` and `minimax-coding-plan` at the same
    // `api` URL, which is drep's own MINIMAX preset. Inserting rather than
    // merging silently discarded one list, so whichever arrived second decided
    // which models drep could resolve at all.
    let endpoint = "https://api.minimax.io/anthropic/v1";
    let document = format!(
        r#"{{"minimax": {{"api": "{endpoint}", "models": {{"MiniMax-M2": {{"id": "MiniMax-M2",
             "temperature": true, "limit": {{"output": 128000}} }} }} }},
           "minimax-coding-plan": {{"api": "{endpoint}", "models": {{"MiniMax-M3": {{"id":
             "MiniMax-M3", "temperature": true, "limit": {{"output": 128000}} }} }} }} }}"#
    );
    let registry = Registry::distil(&document, 0).expect("distils");

    let defaults = Quirks {
        temperature: Some(0.2),
        max_tokens: None,
        max_tokens_from_registry: false,
    };

    for model in ["MiniMax-M2", "MiniMax-M3"] {
        assert_eq!(
            resolve(Some(&registry), defaults, endpoint, model).temperature,
            Some(0.2),
            "`{model}` should be known: neither provider's list may be dropped"
        );
    }
}

#[test]
fn a_published_limit_equal_to_the_fallback_is_not_credited_to_the_registry() {
    // The boundary of "did the registry lower it". Equal is not lower, so the
    // rendered comment must not claim the model's own limit supplied a value
    // the preset already had.
    let registry = one_model(KIMI, "k3", false, Some(200_000));

    let resolved = resolve(Some(&registry), kimi_defaults(), KIMI, "k3");

    assert_eq!(resolved.max_tokens, Some(200_000));
    assert!(
        !resolved.max_tokens_from_registry,
        "equal is not lower: the registry changed nothing"
    );
}
