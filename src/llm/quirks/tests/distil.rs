//! Turning models.dev's document into the two facts drep reads.

use super::super::Registry;
use super::{DOCUMENT, NO_ENDPOINTS};

fn distilled() -> Registry {
    Registry::distil(DOCUMENT, 1_000).expect("the fixture document distils")
}

#[test]
fn a_providers_models_are_reachable_by_its_endpoint() {
    let registry = distilled();

    let k3 = registry
        .facts("https://api.kimi.com/coding/v1", "k3")
        .expect("k3 is in the document");

    assert!(!k3.temperature, "k3 refuses the parameter");
    assert_eq!(k3.output_limit, Some(131_072));
}

#[test]
fn each_model_keeps_its_own_facts_rather_than_its_providers() {
    // Two models under one endpoint with different ceilings. A distillation
    // that folded a provider's models together - or kept only the first - would
    // hand `kimi-for-coding` k3's 131,072, which is four times its own limit.
    let registry = distilled();

    assert_eq!(
        registry
            .facts("https://api.kimi.com/coding/v1", "kimi-for-coding")
            .and_then(|facts| facts.output_limit),
        Some(32_768)
    );
}

#[test]
fn an_endpoint_matches_however_it_was_typed() {
    // The user types the endpoint at the wizard's prompt, and `drep.toml`
    // carries whatever they typed. Matching it byte-for-byte against the
    // vendor's own spelling would miss on a trailing slash.
    let registry = distilled();

    for typed in [
        "https://api.kimi.com/coding/v1",
        "https://api.kimi.com/coding/v1/",
        "HTTPS://API.KIMI.COM/coding/v1",
        "  https://api.kimi.com/coding/v1  ",
    ] {
        assert!(
            registry.facts(typed, "k3").is_some(),
            "`{typed}` should reach the same provider"
        );
    }
}

#[test]
fn a_path_that_differs_is_a_different_provider() {
    // Normalising is not lowercasing: a host serving `/coding/v1` and `/v1`
    // serves two different plans, and collapsing them would hand one
    // endpoint's facts to the other.
    let registry = distilled();

    assert!(registry.facts("https://api.kimi.com/v1", "k3").is_none());
}

#[test]
fn a_provider_with_no_endpoint_is_dropped_rather_than_keyed_by_its_name() {
    // models.dev publishes `openai` with `api: null`. Filed under the vendor id
    // instead, it would be matched by anything that happened to normalise to
    // "openai" - and a model id is not an identity, which is the mistake
    // `Provider::cache_key` exists to avoid.
    let registry = distilled();

    assert!(registry.facts("openai", "gpt-5.6-sol").is_none());
    assert!(
        registry
            .facts("https://api.openai.com/v1", "gpt-5.6-sol")
            .is_none(),
        "drep never learns this endpoint from the document"
    );
}

#[test]
fn a_blank_endpoint_is_dropped_too() {
    // An `api` of whitespace would normalise to the empty string and then match
    // an empty endpoint, which is the one value `config::load` cannot reject
    // early enough to make harmless.
    let registry = distilled();

    assert!(registry.facts("", "nowhere").is_none());
    assert!(registry.facts("   ", "nowhere").is_none());
}

#[test]
fn a_model_that_says_nothing_about_temperature_keeps_it() {
    // 448 of models.dev's entries omit the field. Silence is not evidence that
    // a model refuses the parameter, and withdrawing it on silence would strip
    // `temperature` from most of the index.
    let registry = distilled();

    let quiet = registry
        .facts("https://quiet.example/v1", "unspecified")
        .expect("the model is listed");

    assert!(quiet.temperature, "an unstated temperature is allowed");
    assert_eq!(quiet.output_limit, None, "and no ceiling is invented");
}

#[test]
fn a_model_the_document_does_not_list_is_simply_absent() {
    // A model released after the cache was written. The caller falls back to
    // the preset, which is what `drep init` wrote before any of this existed.
    let registry = distilled();

    assert!(
        registry
            .facts("https://api.kimi.com/coding/v1", "k4-released-today")
            .is_none()
    );
}

#[test]
fn a_document_naming_no_endpoint_at_all_is_malformed() {
    // Not an empty registry silently accepted: a document drep can join nothing
    // against is one it failed to read, and reporting it lets the cache stay
    // unwritten so the next run tries again.
    let err = Registry::distil(NO_ENDPOINTS, 0).expect_err("nothing to join on");

    assert!(err.to_string().contains("named no provider"), "got {err}");
}

#[test]
fn a_body_that_is_not_json_is_malformed() {
    let err = Registry::distil("<html>404</html>", 0).expect_err("not a document");

    assert!(err.to_string().contains("could not be read"), "got {err}");
}

#[test]
fn the_distilled_registry_records_when_it_was_made() {
    // The timestamp is what decides a refetch, so it has to be the one the
    // caller passed rather than anything read from the document.
    assert!(
        !Registry::distil(DOCUMENT, 1_000)
            .expect("distils")
            .is_stale(1_000)
    );
    assert!(
        Registry::distil(DOCUMENT, 1_000)
            .expect("distils")
            .is_stale(1_000 + 8 * 24 * 60 * 60)
    );
}
