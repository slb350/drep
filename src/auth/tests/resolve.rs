//! Filling in the keys `drep.toml` left unset, and saying where each came from.

use super::super::*;
use crate::config::LlmConfig;

/// A config of `n` enabled entries, each with an endpoint and no api_key.
fn config_with(endpoints: &[&str]) -> Config {
    Config {
        max_review_rounds: crate::config::DEFAULT_MAX_REVIEW_ROUNDS,
        llm: endpoints
            .iter()
            .map(|endpoint| LlmConfig {
                endpoint: Some((*endpoint).to_string()),
                model: Some("m".to_string()),
                ..LlmConfig::default()
            })
            .collect(),
    }
}

#[tokio::test]
async fn a_stored_key_fills_in_an_entry_that_named_none() {
    let mut config = config_with(&["https://api.kimi.com/coding/v1"]);
    let mut store = AuthStore::new();
    store
        .set("https://api.kimi.com/coding/v1", "k-1")
        .expect("set");

    let sources = resolve(&mut config, &store)
        .await
        .expect("no command to run");

    assert_eq!(config.llm[0].api_key.as_deref(), Some("k-1"));
    assert_eq!(sources, vec![KeySource::Store]);
}

#[tokio::test]
async fn an_explicit_key_in_the_config_wins_over_a_stored_one() {
    // The user said where the key comes from. Silently preferring a stored one
    // would make the file lie about what the run used.
    let mut config = config_with(&["https://e/v1"]);
    config.llm[0].api_key = Some("from-config".to_string());
    let mut store = AuthStore::new();
    store.set("https://e/v1", "from-store").expect("set");

    let sources = resolve(&mut config, &store)
        .await
        .expect("no command to run");

    assert_eq!(config.llm[0].api_key.as_deref(), Some("from-config"));
    assert_eq!(sources, vec![KeySource::Config]);
}

#[tokio::test]
async fn an_entry_with_no_stored_key_is_left_unset_and_reported_missing() {
    // Not defaulted to anything: `LlmClient::new` applies `not-needed`, which a
    // local server accepts. Inventing a value here would hide that decision.
    let mut config = config_with(&["https://e/v1"]);

    let sources = resolve(&mut config, &AuthStore::new())
        .await
        .expect("no command to run");

    assert_eq!(config.llm[0].api_key, None);
    assert_eq!(sources, vec![KeySource::Missing]);
}

#[tokio::test]
async fn a_disabled_entry_is_skipped_rather_than_resolved() {
    // Every other pass over the provider list leaves a parked entry alone -
    // `${VAR}` expansion and field validation both do. Looking a key up for one
    // would report a missing credential for a provider never contacted.
    let mut config = config_with(&["https://e/v1"]);
    config.llm[0].enabled = false;
    let mut store = AuthStore::new();
    store.set("https://e/v1", "k").expect("set");

    let sources = resolve(&mut config, &store)
        .await
        .expect("no command to run");

    assert_eq!(
        config.llm[0].api_key, None,
        "a parked entry is inert, so nothing was filled in"
    );
    assert_eq!(sources, vec![KeySource::Missing]);
}

#[tokio::test]
async fn sources_are_positional_including_the_disabled_entries() {
    // A caller numbering providers by file position and a caller numbering by
    // chain position both index this, so a skipped entry must occupy its slot
    // rather than being dropped from the list.
    let mut config = config_with(&["https://a/v1", "https://b/v1", "https://c/v1"]);
    config.llm[1].enabled = false;
    config.llm[2].api_key = Some("explicit".to_string());
    let mut store = AuthStore::new();
    store.set("https://a/v1", "k").expect("set");

    let sources = resolve(&mut config, &store)
        .await
        .expect("no command to run");

    assert_eq!(
        sources,
        vec![KeySource::Store, KeySource::Missing, KeySource::Config]
    );
    assert_eq!(sources.len(), config.llm.len());
}

#[tokio::test]
async fn an_entry_with_no_endpoint_resolves_to_missing_rather_than_panicking() {
    // `LlmClient::new` is what rejects a config naming no endpoint, with a
    // message about the endpoint. This pass must not get there first.
    let mut config = Config {
        max_review_rounds: crate::config::DEFAULT_MAX_REVIEW_ROUNDS,
        llm: vec![LlmConfig {
            model: Some("m".to_string()),
            ..LlmConfig::default()
        }],
    };

    assert_eq!(
        resolve(&mut config, &AuthStore::new())
            .await
            .expect("no command to run"),
        vec![KeySource::Missing]
    );
}

#[tokio::test]
async fn resolution_matches_the_endpoint_regardless_of_spelling() {
    let mut config = config_with(&["https://API.Z.AI/api/coding/paas/v4/"]);
    let mut store = AuthStore::new();
    store
        .set("https://api.z.ai/api/coding/paas/v4", "k")
        .expect("set");

    let sources = resolve(&mut config, &store)
        .await
        .expect("no command to run");

    assert_eq!(config.llm[0].api_key.as_deref(), Some("k"));
    assert_eq!(sources, vec![KeySource::Store]);
}

#[test]
fn each_source_has_its_own_label() {
    // `doctor` prints these, and two sources sharing a word would make the line
    // useless for the thing it exists to answer.
    // Driven off `KeySource::ALL` rather than a literal list, so a variant added
    // without wording of its own is a failure here rather than a subset this
    // test quietly stopped covering.
    let labels: Vec<&str> = KeySource::ALL.iter().map(KeySource::label).collect();

    let mut unique = labels.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(
        unique.len(),
        KeySource::ALL.len(),
        "labels collide: {labels:?}"
    );
}
