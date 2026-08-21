//! Narrowing a preset's values against what the registry knows.

use super::super::{Quirks, Registry, resolve};
use super::DOCUMENT;

const KIMI: &str = "https://api.kimi.com/coding/v1";
const ZAI: &str = "https://api.z.ai/api/coding/paas/v4";

fn registry() -> Registry {
    Registry::distil(DOCUMENT, 0).expect("the fixture document distils")
}

/// The `kimi` preset's own values: no temperature, a required `max_tokens`.
///
/// Read from the preset rather than transcribed. Written out as literals these
/// tests kept asserting against numbers the product had moved on from, and they
/// stayed green while doing it - the whole point of `resolve` is what it does
/// to *the preset's* values.
fn kimi_defaults() -> Quirks {
    preset_quirks("kimi")
}

/// The `zai` preset's own values: a temperature, no `max_tokens`.
fn zai_defaults() -> Quirks {
    preset_quirks("zai")
}

fn preset_quirks(key: &str) -> Quirks {
    crate::cli::init::presets::preset(key)
        .expect("the preset exists")
        .quirks()
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
fn a_model_whose_endpoint_publishes_no_limit_keeps_the_presets_fallback() {
    // The endpoint still requires the field, so something has to be written.
    // `quiet.example` is the fixture's provider whose one model publishes no
    // `limit` at all, which is the case a name mentioning the *model* would
    // have described only by accident.
    let defaults = kimi_defaults();

    let resolved = resolve(
        Some(&registry()),
        defaults,
        "https://quiet.example/v1",
        "unspecified",
    );

    assert_eq!(resolved.max_tokens, defaults.max_tokens);
    assert!(
        !resolved.max_tokens_from_registry,
        "the rendered comment must not claim this is the model's own limit"
    );
}

#[test]
fn the_registry_never_introduces_a_max_tokens() {
    // `glm-5.3` publishes an output limit, and z.ai does not require the field.
    // Writing one would put an arbitrary completion cap on a reasoning model.
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
    // `glm-5.2` is the fixture's accepting model on this endpoint, beside
    // `glm-5.3` which refuses - so both directions are exercised against the
    // same provider rather than one being inferred from the other.
    let resolved = resolve(Some(&registry()), zai_defaults(), ZAI, "glm-5.2");

    assert_eq!(resolved.temperature, zai_defaults().temperature);
}

#[test]
fn the_registry_never_introduces_a_temperature() {
    // `zai_defaults` with the temperature taken away stands in for a preset
    // that deliberately sends none. The registry saying the model would accept
    // one is not a reason to start sending it: that is the direction that
    // produces the 400 this feature exists to prevent.
    let mut defaults = zai_defaults();
    defaults.temperature = None;

    let resolved = resolve(Some(&registry()), defaults, ZAI, "glm-5.2");

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

    assert_eq!(
        resolved.max_tokens,
        kimi_defaults().max_tokens,
        "narrowed, never raised"
    );
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
    //
    // Both models refuse a temperature, and the defaults offer one. That is
    // what makes the assertion able to fail: an unknown model returns the
    // defaults untouched, so a dropped list shows up as the 0.2 surviving. An
    // earlier version asserted the temperature was *kept*, which is what
    // `resolve` returns for a known accepting model and for an unknown one
    // alike - it passed whether or not the merge worked.
    let endpoint = "https://api.minimax.io/anthropic/v1";
    let document = format!(
        r#"{{"minimax": {{"api": "{endpoint}", "models": {{"MiniMax-M2": {{"id": "MiniMax-M2",
             "temperature": false, "limit": {{"output": 128000}} }} }} }},
           "minimax-coding-plan": {{"api": "{endpoint}", "models": {{"MiniMax-M3": {{"id":
             "MiniMax-M3", "temperature": false, "limit": {{"output": 128000}} }} }} }} }}"#
    );
    let registry = Registry::distil(&document, 0).expect("distils");

    // A preset that sends a temperature, spelled out because what this test
    // needs is an input the registry can be seen to withdraw - not whatever
    // `zai` happens to send today.
    let defaults = Quirks {
        temperature: Some(0.2),
        max_tokens: None,
        max_tokens_from_registry: false,
    };

    for model in ["MiniMax-M2", "MiniMax-M3"] {
        assert_eq!(
            resolve(Some(&registry), defaults, endpoint, model).temperature,
            None,
            "`{model}` should be known: neither provider's list may be dropped"
        );
    }
}

#[test]
fn a_published_limit_equal_to_the_fallback_is_still_the_models_own_limit() {
    // The boundary, and the one case where "did the registry change the value"
    // and "is this the model's own limit" give different answers. The flag
    // decides a sentence in a file the user commits; with the number equal on
    // both sides that sentence is true, and reading the flag off a `<`
    // comparison made the file say the limit was unknown while printing it.
    let fallback = kimi_defaults().max_tokens.expect("kimi requires the field");
    let registry = one_model(KIMI, "k3", false, Some(fallback));

    let resolved = resolve(Some(&registry), kimi_defaults(), KIMI, "k3");

    assert_eq!(resolved.max_tokens, Some(fallback));
    assert!(
        resolved.max_tokens_from_registry,
        "the registry named this model's limit, so the comment may say so"
    );
}

#[test]
fn a_model_named_without_a_published_limit_is_never_credited_with_one() {
    // The other side of the same flag, and what stops `<=` degenerating into
    // "always true". The registry does name `k3` here - it just publishes no
    // ceiling for it - so the preset's fallback is what gets written, and the
    // rendered file must not claim that number came from the model.
    let registry = one_model(KIMI, "k3", false, None);

    let resolved = resolve(Some(&registry), kimi_defaults(), KIMI, "k3");

    assert_eq!(resolved.max_tokens, kimi_defaults().max_tokens);
    assert!(
        !resolved.max_tokens_from_registry,
        "the registry named no limit, so the fallback is drep's own number"
    );
}
