//! B1, B2: the preset table.

use crate::cli::init::presets;

#[test]
fn preset_keys_are_in_spec_order_and_every_key_round_trips() {
    let keys = presets::preset_keys();
    assert_eq!(
        keys,
        vec!["local", "openrouter", "openai", "custom"],
        "preset_keys() is the source of truth for --provider and --help"
    );
    for key in &keys {
        let p = presets::preset(key).unwrap_or_else(|| panic!("preset({key}) is None"));
        assert_eq!(p.key, *key, "preset key matches what was looked up");
    }
    assert!(presets::preset("nope").is_none());
}

#[test]
fn no_preset_carries_a_max_tokens_assignment() {
    // Pinned at the structural level: `max_tokens =` must not appear at the
    // start of any line, in any preset's rendered config. A test that only
    // checked the rendered body of one preset would miss a regression that
    // re-introduced the field for a single provider.
    use crate::cli::init::presets::PRESETS;
    for preset in PRESETS {
        for (model, endpoint) in [("m", "http://e/v1"), ("x", "https://api.openai.com/v1")] {
            let body = crate::cli::init::config_file::render(preset, model, endpoint);
            for line in body.lines() {
                let trimmed = line.trim_start();
                assert!(
                    !trimmed.starts_with("max_tokens ="),
                    "preset `{}` rendered a max_tokens line: {trimmed:?}",
                    preset.key
                );
            }
        }
    }
}

#[test]
fn openrouter_has_api_key_and_timeout_secs() {
    let preset = presets::preset("openrouter").expect("openrouter");
    assert_eq!(preset.api_key_env, Some("OPENROUTER_API_KEY"));
    assert_eq!(preset.timeout_secs, Some(1800));
}

#[test]
fn local_has_neither_api_key_nor_timeout_secs() {
    let preset = presets::preset("local").expect("local");
    assert_eq!(preset.api_key_env, None);
    assert_eq!(preset.timeout_secs, None);
}

/// Every preset's fields are pinned, not just the two the other tests happen
/// to render.
///
/// `openai` and `custom` were unpinned entirely, so a typo in an endpoint, a
/// default model or an api-key variable name shipped silently - and those are
/// exactly the values a user cannot debug, because a wrong endpoint looks like
/// a network problem and a wrong key variable looks like an unset key.
#[test]
fn every_preset_pins_its_endpoint_model_and_key_variable() {
    /// key, endpoint, default model, api-key variable, timeout.
    struct Expected {
        key: &'static str,
        endpoint: Option<&'static str>,
        model: Option<&'static str>,
        api_key_env: Option<&'static str>,
        timeout_secs: Option<u64>,
    }

    let expected = [
        Expected {
            key: "local",
            endpoint: Some("http://localhost:1234/v1"),
            model: Some("qwen3-30b-a3b"),
            api_key_env: None,
            timeout_secs: None,
        },
        Expected {
            key: "openrouter",
            endpoint: Some("https://openrouter.ai/api/v1"),
            model: Some("deepseek/deepseek-v4-pro-0813"),
            api_key_env: Some("OPENROUTER_API_KEY"),
            timeout_secs: Some(1800),
        },
        Expected {
            key: "openai",
            endpoint: Some("https://api.openai.com/v1"),
            model: Some("gpt-5.6-sol"),
            api_key_env: Some("OPENAI_API_KEY"),
            timeout_secs: Some(1800),
        },
        Expected {
            key: "custom",
            endpoint: None,
            model: None,
            api_key_env: Some("LLM_API_KEY"),
            timeout_secs: None,
        },
    ];

    for want in expected {
        let key = want.key;
        let preset = presets::preset(key).unwrap_or_else(|| panic!("{key} must exist"));
        assert_eq!(preset.endpoint, want.endpoint, "{key} endpoint");
        assert_eq!(preset.default_model, want.model, "{key} default model");
        assert_eq!(
            preset.api_key_env, want.api_key_env,
            "{key} api key variable"
        );
        assert_eq!(preset.timeout_secs, want.timeout_secs, "{key} timeout");
    }
}

/// The local preset needs no key, and every cloud one does.
///
/// Stated as a property rather than per-preset so a new cloud preset that
/// forgot its key variable is caught: the whole point of the presets is that a
/// user picks a name instead of knowing which variable holds the credential.
#[test]
fn only_the_local_preset_needs_no_api_key_variable() {
    for p in presets::PRESETS {
        if p.key == "local" {
            assert!(p.api_key_env.is_none(), "a local model needs no key");
        } else {
            assert!(
                p.api_key_env.is_some(),
                "{} reaches a remote service, so it must name a key variable",
                p.key
            );
        }
    }
}
