//! OpenAI API and ChatGPT/Codex subscription are distinct init targets.

use crate::auth::AuthStore;
use crate::cli::init::{plan_from_flags, presets};
use crate::config::{BackendKind, ReasoningEffort};

#[test]
fn openai_api_preset_keeps_its_existing_wire_contract() {
    let preset = presets::preset("openai").expect("openai preset");
    assert_eq!(preset.display_name, "OpenAI API");
    assert_eq!(preset.backend_kind(), BackendKind::Http);
    assert_eq!(preset.endpoint(), Some("https://api.openai.com/v1"));
    assert_eq!(preset.default_model, Some("gpt-5.6-sol"));
    assert_eq!(preset.api_key_env(), Some("OPENAI_API_KEY"));
    assert_eq!(preset.protocol_name(), None);
}

#[test]
fn codex_is_a_keyless_endpointless_subscription_preset() {
    let preset = presets::preset("codex").expect("codex preset");
    assert_eq!(preset.display_name, "ChatGPT / Codex subscription");
    assert_eq!(preset.backend_kind(), BackendKind::Codex);
    assert_eq!(preset.endpoint(), None);
    assert_eq!(preset.default_model, Some("gpt-5.6-sol"));
    assert_eq!(preset.api_key_env(), None);
    let codex = preset.codex().expect("Codex-specific preset values");
    assert_eq!(codex.reasoning_effort, Some(ReasoningEffort::High));
    assert_eq!(codex.max_concurrent, 1);
}

#[test]
fn codex_flag_plan_needs_no_endpoint_or_key_store_entry() {
    let mut args = super::support::args();
    args.provider = Some("codex".to_owned());

    let plan = plan_from_flags(&args, &AuthStore::new()).expect("codex flag plan");
    assert!(plan.new_keys.is_empty());
    assert_eq!(plan.choices.len(), 1);
    assert_eq!(plan.choices[0].endpoint(), None);
    assert!(!plan.choices[0].key_in_store());
}

#[test]
fn codex_renderer_writes_only_subscription_fields() {
    let preset = presets::preset("codex").expect("codex preset");
    let body = super::support::render_one(preset, "gpt-5.6-sol", "ignored-http-endpoint");
    let value: toml::Value = toml::from_str(&body).expect("valid TOML");
    let entry = &value["llm"].as_array().expect("array")[0];

    assert_eq!(entry["backend"].as_str(), Some("codex"));
    assert_eq!(entry["model"].as_str(), Some("gpt-5.6-sol"));
    assert_eq!(entry["reasoning_effort"].as_str(), Some("high"));
    assert_eq!(entry["timeout_secs"].as_integer(), Some(1800));
    assert_eq!(entry["max_concurrent"].as_integer(), Some(1));

    for forbidden in [
        "endpoint",
        "api_key",
        "protocol",
        "temperature",
        "max_tokens",
        "max_retries",
    ] {
        assert!(
            entry.get(forbidden).is_none(),
            "{forbidden} leaked into {body}"
        );
    }
    assert!(
        body.contains("ChatGPT/Codex subscription allowance"),
        "the billing boundary must be explicit: {body}"
    );
    assert!(body.contains("`codex login`"), "auth ownership: {body}");
    assert!(
        !body.contains("drep auth list"),
        "a Codex-only config must not imply drep stores its credential: {body}"
    );
}

#[test]
fn existing_codex_config_is_described_as_a_subscription_backend() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("drep.toml");
    std::fs::write(
        &path,
        r#"
[[llm]]
backend = "codex"
model = "gpt-5.6-sol"
reasoning_effort = "high"
"#,
    )
    .expect("write config");

    assert_eq!(
        super::super::describe(&path),
        ["gpt-5.6-sol via ChatGPT/Codex subscription"]
    );
}
