//! Which path `init` takes, and what the flag path decides.
//!
//! These cover the two pure decisions between parsing the arguments and acting
//! on them: whether to run the wizard, and what plan the flags alone produce.

use super::support::args;
use crate::auth::AuthStore;
use crate::cli::init::{HookKind, InitArgs, plan_from_flags, wants_wizard};

#[test]
fn a_terminal_with_no_provider_named_runs_the_wizard() {
    assert!(wants_wizard(&args(), true));
}

#[test]
fn a_pipe_with_no_provider_named_does_not() {
    // A hook or a CI job has no stdin to answer with, and prompting there hangs
    // the command rather than failing it.
    assert!(!wants_wizard(&args(), false));
}

#[test]
fn naming_a_provider_skips_the_wizard_even_on_a_terminal() {
    // `--provider` answers the wizard's first question, so there is nothing left
    // to ask that a flag has not already said. This is what keeps every existing
    // scripted invocation working unchanged.
    let named = InitArgs {
        provider: Some("local".to_string()),
        ..args()
    };

    assert!(!wants_wizard(&named, true));
}

#[test]
fn non_interactive_wins_over_a_terminal() {
    let quiet = InitArgs {
        non_interactive: true,
        ..args()
    };

    assert!(!wants_wizard(&quiet, true));
}

#[test]
fn interactive_wins_over_a_pipe() {
    // The case the flag exists for: a wrapper feeding answers on stdin.
    let forced = InitArgs {
        interactive: true,
        ..args()
    };

    assert!(wants_wizard(&forced, false));
}

#[test]
fn interactive_wins_over_a_named_provider_too() {
    // Explicit beats inference in both directions, so the two flags cannot
    // produce a state where neither path is chosen.
    let forced = InitArgs {
        interactive: true,
        provider: Some("local".to_string()),
        ..args()
    };

    assert!(wants_wizard(&forced, false));
}

#[test]
fn the_flag_path_defaults_to_local_when_no_provider_is_named() {
    // `--provider` has to stay `None`-able for `wants_wizard` to read, so the
    // default moved off the argument and into here. It must still be `local`.
    let plan = plan_from_flags(&args(), &AuthStore::new()).expect("plan");

    assert_eq!(plan.choices.len(), 1);
    assert_eq!(plan.choices[0].preset.key, "local");
}

#[test]
fn the_flag_path_gitignores_by_default_and_no_gitignore_opts_out() {
    // Asserted together, because the flag is a negation and inverting it would
    // otherwise pass whichever case was tested alone.
    let on = plan_from_flags(&args(), &AuthStore::new()).expect("plan");
    let off = plan_from_flags(
        &InitArgs {
            no_gitignore: true,
            ..args()
        },
        &AuthStore::new(),
    )
    .expect("plan");

    assert!(on.gitignore, "the default is to add the entry");
    assert!(!off.gitignore, "and --no-gitignore leaves .gitignore alone");
}

#[test]
fn the_flag_path_carries_the_hook_selection_through() {
    let plan = plan_from_flags(
        &InitArgs {
            hooks: HookKind::Both,
            ..args()
        },
        &AuthStore::new(),
    )
    .expect("plan");

    assert_eq!(plan.hooks, HookKind::Both);
}

#[test]
fn the_flag_path_stores_no_keys() {
    // There is nobody to paste one. A flag run can only ever use a key that is
    // already held or already exported.
    let plan = plan_from_flags(&args(), &AuthStore::new()).expect("plan");

    assert!(plan.new_keys.is_empty());
}

#[test]
fn a_key_already_held_suppresses_the_api_key_line() {
    // An explicit `api_key` wins over the store, so writing `${VAR}` when a key
    // is held would override it with a variable the user may never have set.
    let mut store = AuthStore::new();
    store
        .set("https://openrouter.ai/api/v1", "sk-held")
        .expect("set");

    let plan = plan_from_flags(
        &InitArgs {
            provider: Some("openrouter".to_string()),
            ..args()
        },
        &store,
    )
    .expect("plan");

    assert!(plan.choices[0].key_in_store);
}

#[test]
fn no_key_held_leaves_the_api_key_line_in_place() {
    let plan = plan_from_flags(
        &InitArgs {
            provider: Some("openrouter".to_string()),
            ..args()
        },
        &AuthStore::new(),
    )
    .expect("plan");

    assert!(
        !plan.choices[0].key_in_store,
        "so `api_key = \"${{OPENROUTER_API_KEY}}\"` is written and CI still works"
    );
}

#[test]
fn an_explicit_model_and_endpoint_beat_the_preset() {
    let plan = plan_from_flags(
        &InitArgs {
            provider: Some("openrouter".to_string()),
            model: Some("my-model".to_string()),
            endpoint: Some("https://mine/v1".to_string()),
            ..args()
        },
        &AuthStore::new(),
    )
    .expect("plan");

    assert_eq!(plan.choices[0].model, "my-model");
    assert_eq!(plan.choices[0].endpoint, "https://mine/v1");
}

#[test]
fn a_preset_presuming_no_host_needs_an_endpoint() {
    let err = plan_from_flags(
        &InitArgs {
            provider: Some("custom".to_string()),
            ..args()
        },
        &AuthStore::new(),
    )
    .expect_err("custom names no endpoint");

    assert!(err.to_string().contains("--endpoint"), "got {err}");
}

#[test]
fn a_preset_presuming_no_model_needs_one() {
    let err = plan_from_flags(
        &InitArgs {
            provider: Some("custom".to_string()),
            endpoint: Some("https://mine/v1".to_string()),
            ..args()
        },
        &AuthStore::new(),
    )
    .expect_err("custom names no model");

    assert!(err.to_string().contains("--model"), "got {err}");
}
