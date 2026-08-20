//! The chosen model, not the preset, deciding `temperature` and `max_tokens`.
//!
//! The preset's values are a provider-scoped guess: right for its default model
//! and unverified for every other model that endpoint serves - which is exactly
//! what the wizard lets the user pick.

use super::super::*;
use super::{Catalog, Quirked, Scripted, number_of};

/// A models.dev-shaped document covering the two subscription endpoints these
/// tests configure.
///
/// `glm-5.3` is given `temperature: false` deliberately, so the "the registry
/// withdrew it" case is distinguishable from the `zai` preset's own `Some(0.2)`.
const FIXTURE: &str = r#"{
  "kimi-for-coding": {
    "api": "https://api.kimi.com/coding/v1",
    "models": {
      "k3": { "temperature": false, "limit": { "context": 262144, "output": 131072 } },
      "kimi-for-coding": { "temperature": false, "limit": { "context": 262144, "output": 32768 } }
    }
  },
  "zai-coding-plan": {
    "api": "https://api.z.ai/api/coding/paas/v4",
    "models": {
      "glm-5.3": { "temperature": false, "limit": { "context": 204800, "output": 131072 } },
      "glm-5.2": { "temperature": true, "limit": { "context": 204800, "output": 131072 } }
    }
  }
}"#;

/// Run the wizard for one provider that needs a key, against `quirks`.
///
/// Answers: provider, endpoint default, the pasted key, the model step, no
/// fallback, hooks, gitignore.
async fn run_one(
    provider: &str,
    catalog: &Catalog,
    quirks: &Quirked,
    model_answer: &str,
) -> (Plan, Scripted) {
    let number = number_of(provider);
    let mut console = Scripted::new(&[number.as_str(), "", "sk-pasted", model_answer, "", "", ""]);
    let plan = run(
        &mut console,
        Deps {
            store: &AuthStore::new(),
            source: catalog,
            quirks_source: quirks,
            env_is_set: &|_| false,
        },
    )
    .await
    .expect("the wizard completes");
    assert!(console.is_drained(), "unused answers: the flow differed");
    (plan, console)
}

#[tokio::test]
async fn the_chosen_models_own_limit_replaces_the_presets_required_max_tokens() {
    // `api.kimi.com/coding/v1` refuses a request that omits the field, so
    // something must be written - but the preset's 200,000 is a number nobody
    // checked against the model. `kimi-for-coding` publishes 32,768.
    let catalog = Catalog::of(&["k3", "kimi-for-coding"]);

    let (plan, _) = run_one("kimi", &catalog, &Quirked::from_json(FIXTURE), "2").await;

    assert_eq!(plan.choices[0].model, "kimi-for-coding");
    assert_eq!(plan.choices[0].quirks.max_tokens, Some(32_768));
    assert!(plan.choices[0].quirks.max_tokens_from_registry);
}

#[tokio::test]
async fn a_different_model_on_the_same_endpoint_gets_a_different_limit() {
    // The pair is the point: one preset, two models, two answers. A single
    // provider-scoped value cannot produce both.
    let catalog = Catalog::of(&["k3", "kimi-for-coding"]);

    let (plan, _) = run_one("kimi", &catalog, &Quirked::from_json(FIXTURE), "1").await;

    assert_eq!(plan.choices[0].model, "k3");
    assert_eq!(plan.choices[0].quirks.max_tokens, Some(131_072));
}

#[tokio::test]
async fn a_model_the_registry_does_not_name_keeps_the_presets_value() {
    // Typed at the prompt because it was released this morning. The endpoint
    // still requires the field, so the preset's fallback is what gets written.
    let catalog = Catalog::of(&["k3"]);

    let (plan, _) = run_one("kimi", &catalog, &Quirked::from_json(FIXTURE), "k4-preview").await;

    assert_eq!(plan.choices[0].model, "k4-preview");
    assert_eq!(plan.choices[0].quirks.max_tokens, Some(200_000));
    assert!(
        !plan.choices[0].quirks.max_tokens_from_registry,
        "the rendered comment must not claim this is the model's own limit"
    );
}

#[tokio::test]
async fn an_unreachable_registry_leaves_the_preset_values_untouched_and_says_so() {
    // The non-negotiable one: nothing about model quirks may stop `drep init`.
    let catalog = Catalog::of(&["k3"]);

    let (plan, console) = run_one("kimi", &catalog, &Quirked::Unavailable, "1").await;

    assert_eq!(plan.choices[0].quirks.max_tokens, Some(200_000));
    assert_eq!(plan.choices[0].quirks.temperature, None);
    assert!(
        console
            .transcript()
            .contains("Could not check model quirks"),
        "got {}",
        console.transcript()
    );
}

#[tokio::test]
async fn the_registry_is_consulted_once_for_the_whole_chain() {
    // One document, not one per provider. Reported once too, so a two-provider
    // chain configured offline does not say the same thing twice.
    let catalog = Catalog::of(&["glm-5.3"]);
    let zai = number_of("zai");
    let mut console = Scripted::new(&[
        zai.as_str(),
        "",
        "sk-one",
        "1",
        "y", // add a fallback
        zai.as_str(),
        "",
        "1", // the key is already pending for this endpoint
        "",
        "",
        "",
    ]);

    run(
        &mut console,
        Deps {
            store: &AuthStore::new(),
            source: &catalog,
            quirks_source: &Quirked::Unavailable,
            env_is_set: &|_| false,
        },
    )
    .await
    .expect("the wizard completes");

    assert!(console.is_drained(), "unused answers: the flow differed");
    assert_eq!(
        console
            .transcript()
            .matches("Could not check model quirks")
            .count(),
        1
    );
}

#[tokio::test]
async fn a_model_that_refuses_temperature_loses_it_even_when_the_preset_sends_one() {
    // `zai` sends 0.2, because `glm-5.3` accepted it when the preset was
    // written. A model on the same plan that refuses it would answer a 400, and
    // a 400 neither fails over nor retries.
    let catalog = Catalog::of(&["glm-5.3", "glm-5.2"]);

    let (plan, _) = run_one("zai", &catalog, &Quirked::from_json(FIXTURE), "1").await;

    assert_eq!(plan.choices[0].model, "glm-5.3");
    assert_eq!(plan.choices[0].quirks.temperature, None);
}

#[tokio::test]
async fn a_model_that_accepts_temperature_keeps_the_presets_value() {
    let catalog = Catalog::of(&["glm-5.3", "glm-5.2"]);

    let (plan, _) = run_one("zai", &catalog, &Quirked::from_json(FIXTURE), "2").await;

    assert_eq!(plan.choices[0].model, "glm-5.2");
    assert_eq!(plan.choices[0].quirks.temperature, Some(0.2));
}

#[tokio::test]
async fn the_registry_never_adds_a_max_tokens_to_an_endpoint_that_does_not_need_one() {
    // `glm-5.2` publishes an output limit and z.ai accepts a request without
    // the field. Writing one would put a completion cap on a reasoning model,
    // which is the coupling 2.0 removed.
    let catalog = Catalog::of(&["glm-5.3", "glm-5.2"]);

    let (plan, _) = run_one("zai", &catalog, &Quirked::from_json(FIXTURE), "2").await;

    assert_eq!(plan.choices[0].quirks.max_tokens, None);
}

#[tokio::test]
async fn the_written_config_carries_the_resolved_values_not_the_presets() {
    // Through the renderer, because a `Plan` nobody writes out is not a config.
    let catalog = Catalog::of(&["k3", "kimi-for-coding"]);

    let (plan, _) = run_one("kimi", &catalog, &Quirked::from_json(FIXTURE), "2").await;
    let body = crate::cli::init::config_file::render_chain(&plan.choices);

    assert!(body.contains("max_tokens = 32768"), "got {body}");
    assert!(
        !body.contains("200000"),
        "the preset's guess is gone: {body}"
    );
    assert!(
        body.contains("the model's own published output limit"),
        "and the comment says where it came from: {body}"
    );
}
