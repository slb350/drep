//! Navigation: which provider, which model, the fallback chain, hooks and
//! `.gitignore`.

use super::super::*;
use super::{Catalog, Quirked, Scripted, number_of};

/// Run the wizard against `answers` with an empty store.
///
/// The endpoint serves no listing, so the model step is the free-text prompt.
/// The listing path has its own file; keeping it out of the way here means
/// these scripts read as the navigation they are testing.
async fn run_with(answers: &[&str]) -> (Plan, Scripted) {
    let mut console = Scripted::new(answers);
    let plan = run(
        &mut console,
        Deps {
            store: &AuthStore::new(),
            source: &Catalog::Unavailable,
            quirks_source: &Quirked::Unavailable,
            env_is_set: &|_| false,
        },
    )
    .await
    .expect("the wizard completes");
    assert!(
        console.is_drained(),
        "the script left answers unused, so this asserts against a flow that did not happen"
    );
    (plan, console)
}

#[tokio::test]
async fn accepting_every_default_configures_the_local_provider() {
    // provider, endpoint, model, no fallback, hooks, gitignore
    let (plan, _) = run_with(&["", "", "", "", "", ""]).await;

    assert_eq!(plan.choices.len(), 1);
    assert_eq!(plan.choices[0].preset.key, "local");
    assert_eq!(plan.hooks, HookKind::PrePush);
    assert!(plan.gitignore, "the default answer is yes");
    assert!(plan.new_keys.is_empty(), "local needs no key");
}

#[tokio::test]
async fn the_chosen_number_selects_that_preset() {
    let (plan, _) = run_with(&[&number_of("zai"), "", "", "", "", "", ""]).await;

    assert_eq!(plan.choices[0].preset.key, "zai");
    assert_eq!(
        plan.choices[0].endpoint,
        "https://api.z.ai/api/coding/paas/v4"
    );
    assert_eq!(plan.choices[0].model, "glm-5.3");
}

#[tokio::test]
async fn an_out_of_range_number_is_re_asked_rather_than_accepted() {
    // Selecting past the end of the table would panic on the index, and
    // selecting zero would silently pick the last entry under wrapping
    // arithmetic. Both have to become another question.
    let (plan, console) = run_with(&["99", "0", "not a number", "1", "", "", "", "", ""]).await;

    assert_eq!(plan.choices[0].preset.key, presets::PRESETS[0].key);
    assert!(
        console.transcript().contains("Enter a number from 1 to"),
        "the user was told what is valid: {}",
        console.transcript()
    );
}

#[tokio::test]
async fn an_overridden_model_and_endpoint_reach_the_choice() {
    let (plan, _) = run_with(&["1", "http://elsewhere:9/v1", "some-model", "", "", ""]).await;

    assert_eq!(plan.choices[0].endpoint, "http://elsewhere:9/v1");
    assert_eq!(plan.choices[0].model, "some-model");
}

#[tokio::test]
async fn a_preset_with_no_default_endpoint_re_asks_until_one_is_given() {
    // `custom` presumes no host, so an empty answer has nothing to fall back on.
    // Accepting it would write a config `config::load` rejects, from a command
    // that just reported success.
    let (plan, console) = run_with(&[
        &number_of("custom"),
        "",
        "   ",
        "https://mine/v1",
        "",         // no key pasted
        "my-model", // the model, asked after the key
        "",         // no fallback
        "",         // hooks
        "",         // gitignore
    ])
    .await;

    assert_eq!(plan.choices[0].endpoint, "https://mine/v1");
    assert!(
        console.transcript().contains("Endpoint cannot be empty"),
        "got {}",
        console.transcript()
    );
}

#[tokio::test]
async fn a_fallback_becomes_a_second_entry_in_chain_order() {
    // The order is the failover chain, so which answer lands first is the whole
    // meaning of the list.
    let (plan, _) = run_with(&[
        "1",
        "",
        "",  // local
        "y", // add a fallback
        &number_of("openrouter"),
        "",
        "", // its endpoint and model
        "", // no key pasted
        "", // no further fallback
        "",
        "", // hooks, gitignore
    ])
    .await;

    assert_eq!(plan.choices.len(), 2);
    assert_eq!(plan.choices[0].preset.key, "local");
    assert_eq!(plan.choices[1].preset.key, "openrouter");
}

#[tokio::test]
async fn a_third_provider_is_reachable_too() {
    let (plan, _) = run_with(&[
        "1", "", "", "y", // local, then another
        "1", "", "", "y", // and another
        "1", "", "", "n", // and stop
        "", "",
    ])
    .await;

    assert_eq!(plan.choices.len(), 3);
}

#[tokio::test]
async fn the_fallback_question_defaults_to_no() {
    // The common case is one provider. Defaulting to yes would make Enter loop.
    let (plan, _) = run_with(&["1", "", "", "", "", ""]).await;

    assert_eq!(plan.choices.len(), 1);
}

#[tokio::test]
async fn each_hook_number_maps_to_its_own_kind() {
    let table = [
        ("1", HookKind::PrePush),
        ("2", HookKind::PreCommit),
        ("3", HookKind::Both),
        ("4", HookKind::None),
    ];

    for (answer, expected) in table {
        let (plan, _) = run_with(&["1", "", "", "", answer, ""]).await;
        assert_eq!(plan.hooks, expected, "answer {answer}");
    }
}

#[tokio::test]
async fn an_invalid_hook_number_is_re_asked() {
    let (plan, console) = run_with(&["1", "", "", "", "9", "2", ""]).await;

    assert_eq!(plan.hooks, HookKind::PreCommit);
    assert!(console.transcript().contains("Enter a number from 1 to 4"));
}

#[tokio::test]
async fn the_gitignore_question_answers_both_ways() {
    for (answer, expected) in [("y", true), ("n", false), ("", true)] {
        let (plan, _) = run_with(&["1", "", "", "", "", answer]).await;
        assert_eq!(plan.gitignore, expected, "answer {answer:?}");
    }
}

#[tokio::test]
async fn a_yes_no_question_accepts_long_forms_and_case() {
    for (answer, expected) in [("YES", true), ("No", false), ("yes", true), ("N", false)] {
        let (plan, _) = run_with(&["1", "", "", "", "", answer]).await;
        assert_eq!(plan.gitignore, expected, "answer {answer:?}");
    }
}

#[tokio::test]
async fn an_unparseable_yes_no_answer_is_re_asked() {
    let (plan, console) = run_with(&["1", "", "", "", "", "maybe", "n"]).await;

    assert!(!plan.gitignore);
    assert!(console.transcript().contains("Enter y or n"));
}

#[tokio::test]
async fn the_provider_list_shows_every_preset_with_its_description() {
    // The list is the only place a user learns what the options are, so a
    // truncated one is a preset nobody can pick.
    let (_, console) = run_with(&["1", "", "", "", "", ""]).await;
    let transcript = console.transcript();

    for preset in presets::PRESETS {
        assert!(
            transcript.contains(preset.display_name),
            "`{}` is missing from the list: {transcript}",
            preset.key
        );
        assert!(
            transcript.contains(preset.description),
            "`{}` has no description in the list",
            preset.key
        );
    }
}

#[tokio::test]
async fn the_fallback_prompt_names_which_fallback_it_is_asking_about() {
    // With three providers configured, "Which provider?" three times gives no
    // indication of where in the chain the answer lands.
    let (_, console) = run_with(&["1", "", "", "y", "1", "", "", "n", "", ""]).await;

    assert!(
        console.transcript().contains("fallback #1"),
        "got {}",
        console.transcript()
    );
}
