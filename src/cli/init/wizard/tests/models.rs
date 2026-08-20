//! Choosing a model from what the endpoint says it serves.
//!
//! The prompt this replaced offered a hardcoded name and checked nothing, so a
//! typo or a model outside the plan surfaced as a 404 on the first push.

use super::super::*;
use super::{Catalog, Quirked, Recording, Scripted, deps, number_of};

/// Run the wizard for one `local` provider against `catalog`.
///
/// `local` needs no key, so the answers are: provider, endpoint, the model
/// step, no fallback, hooks, gitignore.
async fn run_local(catalog: &Catalog, model_answers: &[&str]) -> (Plan, Scripted) {
    let mut answers = vec!["1", ""];
    answers.extend_from_slice(model_answers);
    answers.extend_from_slice(&["", "", ""]);

    let mut console = Scripted::new(&answers);
    let plan = run(
        &mut console,
        deps(&AuthStore::new(), catalog, &Quirked::Unavailable),
    )
    .await
    .expect("the wizard completes");
    assert!(console.is_drained(), "unused answers: the flow differed");
    (plan, console)
}

#[tokio::test]
async fn the_endpoints_models_are_offered_and_a_number_picks_one() {
    let catalog = Catalog::of(&["glm-5.3", "glm-5.2", "glm-4.7"]);

    let (plan, console) = run_local(&catalog, &["2"]).await;

    assert_eq!(plan.choices[0].model, "glm-5.2");
    let transcript = console.transcript();
    for id in ["glm-5.3", "glm-5.2", "glm-4.7"] {
        assert!(
            transcript.contains(id),
            "`{id}` was not offered: {transcript}"
        );
    }
}

#[tokio::test]
async fn the_presets_default_is_preselected_when_the_endpoint_still_serves_it() {
    // `local` defaults to qwen3-30b-a3b. Offered second, so accepting the
    // default has to yield it rather than the first entry.
    let catalog = Catalog::of(&["something-else", "qwen3-30b-a3b"]);

    let (plan, _) = run_local(&catalog, &[""]).await;

    assert_eq!(plan.choices[0].model, "qwen3-30b-a3b");
}

#[tokio::test]
async fn a_default_the_endpoint_no_longer_serves_is_called_out() {
    // This is the failure the listing exists to remove: a shipped default that
    // has gone stale. Silently preselecting the first entry instead would hide
    // that the preset needs updating.
    let catalog = Catalog::of(&["glm-5.3", "glm-5.2"]);

    let (plan, console) = run_local(&catalog, &["1"]).await;

    assert_eq!(plan.choices[0].model, "glm-5.3");
    assert!(
        console.transcript().contains("qwen3-30b-a3b"),
        "the absent default should be named: {}",
        console.transcript()
    );
    assert!(
        console.transcript().contains("not in this list"),
        "got {}",
        console.transcript()
    );
}

#[tokio::test]
async fn a_name_outside_the_list_is_accepted() {
    // A model released this morning is exactly the one somebody is trying to
    // configure. A menu that refused it would be worse than the free-text
    // prompt it replaced.
    let catalog = Catalog::of(&["glm-5.3"]);

    let (plan, _) = run_local(&catalog, &["glm-6-preview"]).await;

    assert_eq!(plan.choices[0].model, "glm-6-preview");
}

#[tokio::test]
async fn a_number_outside_the_list_is_re_asked_rather_than_taken_as_a_name() {
    // `9` against a three-entry list is a misread menu, not a model called
    // "9". Accepting it would write a config that 404s on the first push.
    let catalog = Catalog::of(&["a", "b", "c"]);

    let (plan, console) = run_local(&catalog, &["9", "0", "2"]).await;

    assert_eq!(plan.choices[0].model, "b");
    assert!(
        console.transcript().contains("Enter a number from 1 to 3"),
        "got {}",
        console.transcript()
    );
}

#[tokio::test]
async fn a_display_name_is_shown_beside_the_id() {
    // `kimi-for-coding` is "K2.7 Coding" to its own vendor, which nobody would
    // guess from the id they have to put in the config.
    let catalog = Catalog::Serves(vec![crate::llm::models::Model {
        id: "kimi-for-coding".to_string(),
        display_name: Some("K2.7 Coding".to_string()),
    }]);

    let (_, console) = run_local(&catalog, &["1"]).await;

    assert!(
        console
            .transcript()
            .contains("kimi-for-coding (K2.7 Coding)"),
        "got {}",
        console.transcript()
    );
}

#[tokio::test]
async fn an_endpoint_with_no_listing_falls_back_to_typing_a_name() {
    // A local llama.cpp build, a gateway, anything older. Setup has to continue
    // exactly as it did before the listing existed.
    let (plan, console) = run_local(&Catalog::Unavailable, &["typed-by-hand"]).await;

    assert_eq!(plan.choices[0].model, "typed-by-hand");
    assert!(
        console.transcript().contains("Could not list models"),
        "and says why: {}",
        console.transcript()
    );
}

#[tokio::test]
async fn the_fallback_still_offers_the_presets_default() {
    let (plan, _) = run_local(&Catalog::Unavailable, &[""]).await;

    assert_eq!(plan.choices[0].model, "qwen3-30b-a3b");
}

#[tokio::test]
async fn a_rejected_key_falls_back_rather_than_failing_the_wizard() {
    // Nothing about a listing may stop setup. The reason is reported, because
    // the user is about to store a key that did not work.
    let (plan, console) = run_local(&Catalog::Rejected, &["typed-anyway"]).await;

    assert_eq!(plan.choices[0].model, "typed-anyway");
    assert!(
        console.transcript().contains("rejected the key"),
        "got {}",
        console.transcript()
    );
}

#[tokio::test]
async fn a_pasted_key_authenticates_the_listing() {
    // The reason the key step moved ahead of the model step. Listing with an
    // empty key would 401 on every provider that needs one, so the menu would
    // never appear for exactly the endpoints it was built for.
    let source = Recording::new(Catalog::of(&["glm-5.3"]));
    let zai = number_of("zai");
    let mut console = Scripted::new(&[
        zai.as_str(),
        "",          // endpoint default
        "sk-pasted", // the key
        "1",         // pick the listed model
        "",          // no fallback
        "",
        "",
    ]);

    run(
        &mut console,
        deps(&AuthStore::new(), &source, &Quirked::Unavailable),
    )
    .await
    .expect("the wizard completes");

    let calls = source.calls.borrow();
    assert_eq!(calls.len(), 1, "one listing per provider");
    assert_eq!(calls[0].0, "https://api.z.ai/api/coding/paas/v4");
    assert_eq!(
        calls[0].1, "sk-pasted",
        "the pasted key reached the listing"
    );
}

#[tokio::test]
async fn a_key_already_in_the_store_authenticates_the_listing_too() {
    let mut store = AuthStore::new();
    store
        .set("https://api.z.ai/api/coding/paas/v4", "sk-stored")
        .expect("set");

    let source = Recording::new(Catalog::of(&["glm-5.3"]));
    let zai = number_of("zai");
    let mut console = Scripted::new(&[zai.as_str(), "", "1", "", "", ""]);

    run(&mut console, deps(&store, &source, &Quirked::Unavailable))
        .await
        .expect("the wizard completes");

    assert_eq!(source.calls.borrow()[0].1, "sk-stored");
}

#[tokio::test]
async fn a_provider_needing_no_key_lists_unauthenticated() {
    // A local server serves its catalogue without a key, and passing a
    // placeholder would be a header it has no use for.
    let source = Recording::new(Catalog::of(&["qwen3-30b-a3b"]));
    let mut console = Scripted::new(&["1", "", "1", "", "", ""]);

    run(
        &mut console,
        deps(&AuthStore::new(), &source, &Quirked::Unavailable),
    )
    .await
    .expect("the wizard completes");

    assert_eq!(source.calls.borrow()[0].1, "");
}
