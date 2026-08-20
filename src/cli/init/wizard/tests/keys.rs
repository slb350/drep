//! Where a provider's key comes from: pasted, already stored, or left to the
//! environment.

use super::super::*;
use super::{Catalog, Scripted, number_of};

/// Run the wizard for one `zai` provider, with `key_answer` given at the paste
/// prompt, against `store`.
///
/// The key is asked *before* the model, because the model listing needs a key
/// to authenticate with. This endpoint serves no listing, so the model step is
/// the free-text prompt.
async fn run_zai(store: &AuthStore, key_answer: &str) -> (Plan, Scripted) {
    let provider = number_of("zai");
    let answers = vec![
        provider.as_str(),
        "",         // endpoint default
        key_answer, // the paste prompt
        "",         // model default
        "",         // no fallback
        "",         // hooks
        "",         // gitignore
    ];
    let mut console = Scripted::new(&answers);
    let plan = run(&mut console, store, &Catalog::Unavailable)
        .await
        .expect("the wizard completes");
    assert!(console.is_drained(), "unused answers: the flow differed");
    (plan, console)
}

#[tokio::test]
async fn a_pasted_key_is_queued_for_the_store_and_kept_out_of_the_config() {
    let (plan, _) = run_zai(&AuthStore::new(), "sk-pasted").await;

    assert_eq!(
        plan.new_keys,
        vec![(
            "https://api.z.ai/api/coding/paas/v4".to_string(),
            "sk-pasted".to_string()
        )]
    );
    assert!(
        plan.choices[0].key_in_store,
        "so no api_key line is rendered: an explicit one would override the key just saved"
    );
}

#[tokio::test]
async fn the_key_prompt_is_a_secret_prompt() {
    // The one credential drep would otherwise leave in the clear. Reading it as
    // an ordinary line puts it in the terminal's scrollback.
    let (_, console) = run_zai(&AuthStore::new(), "sk-pasted").await;

    assert_eq!(
        console.secrets_asked.len(),
        1,
        "expected exactly one secret prompt, got {:?}",
        console.secrets_asked
    );
    assert!(
        console.secrets_asked[0].contains("Paste your API key"),
        "got {:?}",
        console.secrets_asked
    );
}

#[tokio::test]
async fn an_empty_paste_falls_back_to_the_environment_variable() {
    let (plan, console) = run_zai(&AuthStore::new(), "").await;

    assert!(plan.new_keys.is_empty(), "nothing to store");
    assert!(
        !plan.choices[0].key_in_store,
        "so `api_key = \"${{ZAI_API_KEY}}\"` is written instead"
    );
    assert!(
        console.transcript().contains("ZAI_API_KEY"),
        "and the variable is named: {}",
        console.transcript()
    );
}

#[tokio::test]
async fn whitespace_only_is_treated_as_an_empty_paste() {
    // A stray space before Enter must not be stored as a key, which would
    // satisfy every "is a key present" check and then 401 at the endpoint.
    let (plan, _) = run_zai(&AuthStore::new(), "   ").await;

    assert!(plan.new_keys.is_empty());
    assert!(!plan.choices[0].key_in_store);
}

#[tokio::test]
async fn a_key_already_in_the_store_is_reused_without_asking() {
    // Asking again invites overwriting a working key with a typo.
    let mut store = AuthStore::new();
    store
        .set("https://api.z.ai/api/coding/paas/v4", "sk-existing")
        .expect("set");

    let provider = number_of("zai");
    let mut console = Scripted::new(&[provider.as_str(), "", "", "", "", ""]);
    let plan = run(&mut console, &store, &Catalog::Unavailable)
        .await
        .expect("the wizard completes");

    assert!(console.is_drained(), "no paste prompt was answered");
    assert!(
        console.secrets_asked.is_empty(),
        "nothing should have been asked as a secret: {:?}",
        console.secrets_asked
    );
    assert!(plan.choices[0].key_in_store);
    assert!(plan.new_keys.is_empty());
    assert!(console.transcript().contains("already stored"));
}

#[tokio::test]
async fn a_stored_key_is_found_regardless_of_a_trailing_slash() {
    // The store normalises; the wizard has to look it up the same way or it
    // would ask for a key the machine already holds.
    let mut store = AuthStore::new();
    store
        .set("https://api.z.ai/api/coding/paas/v4/", "sk-existing")
        .expect("set");

    let provider = number_of("zai");
    let mut console = Scripted::new(&[provider.as_str(), "", "", "", "", ""]);
    let plan = run(&mut console, &store, &Catalog::Unavailable)
        .await
        .expect("the wizard completes");

    assert!(console.secrets_asked.is_empty());
    assert!(plan.choices[0].key_in_store);
}

#[tokio::test]
async fn a_key_pasted_earlier_in_the_same_run_is_not_asked_for_twice() {
    // Two providers can share an endpoint - a different model on the same host
    // is the obvious case. The second must reuse what the first just pasted,
    // since the store has not been written yet.
    let zai = number_of("zai");
    let mut console = Scripted::new(&[
        zai.as_str(),
        "",          // endpoint default
        "sk-pasted", // first: paste the key
        "",          // its model
        "y",         // add a fallback
        zai.as_str(),
        "",        // same endpoint, so no key prompt
        "glm-5.2", // a different model on it
        "",        // no further fallback
        "",
        "",
    ]);

    let plan = run(&mut console, &AuthStore::new(), &Catalog::Unavailable)
        .await
        .expect("the wizard completes");

    assert!(
        console.is_drained(),
        "the second provider asked for a key again"
    );
    assert_eq!(console.secrets_asked.len(), 1, "asked once, not twice");
    assert_eq!(plan.new_keys.len(), 1, "and queued once");
    assert!(plan.choices[1].key_in_store, "the second entry uses it too");
}

#[tokio::test]
async fn a_provider_needing_no_key_is_never_asked_for_one() {
    let mut console = Scripted::new(&["1", "", "", "", "", ""]);
    let plan = run(&mut console, &AuthStore::new(), &Catalog::Unavailable)
        .await
        .expect("the wizard completes");

    assert_eq!(plan.choices[0].preset.key, "local");
    assert!(
        console.secrets_asked.is_empty(),
        "a local server has no key to paste"
    );
    assert!(!plan.choices[0].key_in_store);
}

#[tokio::test]
async fn the_place_to_get_a_key_is_shown_when_the_preset_knows_it() {
    let (_, console) = run_zai(&AuthStore::new(), "").await;

    let url = presets::preset("zai")
        .expect("preset")
        .key_url
        .expect("url");
    assert!(
        console.transcript().contains(url),
        "the wizard should say where to get a key: {}",
        console.transcript()
    );
}

#[tokio::test]
async fn every_preset_that_needs_a_key_knows_where_to_get_one() {
    // The guidance is the reason the wizard exists. A preset that demands a key
    // and cannot say where it comes from sends the user to a search engine.
    for preset in presets::PRESETS {
        if preset.api_key_env.is_some() && preset.endpoint.is_some() {
            assert!(
                preset.key_url.is_some(),
                "preset `{}` needs a key but names no source",
                preset.key
            );
        }
    }
}

#[tokio::test]
async fn an_already_exported_variable_is_reported_at_the_prompt() {
    // It changes what the empty answer means: with the variable exported,
    // skipping the paste is a complete setup rather than a deferred one.
    //
    // SAFETY: single-threaded test process, and the variable is removed below.
    unsafe { std::env::set_var("ZAI_API_KEY", "already-here") };

    let (_, console) = run_zai(&AuthStore::new(), "").await;
    let transcript = console.transcript();

    unsafe { std::env::remove_var("ZAI_API_KEY") };

    assert!(
        transcript.contains("already set in this shell"),
        "got {transcript}"
    );
}
