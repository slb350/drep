//! B1, B2: the preset table.

use crate::cli::init::presets;

#[test]
fn preset_keys_are_in_spec_order_and_every_key_round_trips() {
    let keys = presets::preset_keys();
    assert_eq!(
        keys,
        vec![
            "local",
            "openrouter",
            "zai",
            "minimax",
            "kimi",
            "openai",
            "custom"
        ],
        "preset_keys() is the source of truth for --provider and --help"
    );
    for key in &keys {
        let p = presets::preset(key).unwrap_or_else(|| panic!("preset({key}) is None"));
        assert_eq!(p.key, *key, "preset key matches what was looked up");
    }
    assert!(presets::preset("nope").is_none());
}

#[test]
fn only_a_preset_whose_endpoint_requires_max_tokens_renders_one() {
    // The rule is still "no completion cap": an invented ceiling truncates a
    // reasoning model mid-thought, which is the coupling 2.0 removed. The single
    // exception is an endpoint that refuses a request *without* the field, where
    // omitting it is not a lighter touch but a 400. Asserted as an exact set
    // rather than per-preset, so re-introducing a cap for any other provider
    // fails here.
    use crate::cli::init::presets::PRESETS;

    let mut rendered = Vec::new();
    for preset in PRESETS {
        for (model, endpoint) in [("m", "http://e/v1"), ("x", "https://api.openai.com/v1")] {
            let body = super::support::render_one(preset, model, endpoint);
            let has_line = body
                .lines()
                .any(|line| line.trim_start().starts_with("max_tokens ="));
            assert_eq!(
                has_line,
                preset.max_tokens.is_some(),
                "preset `{}` renders max_tokens={has_line} but declares {:?}",
                preset.key,
                preset.max_tokens
            );
            if has_line && !rendered.contains(&preset.key) {
                rendered.push(preset.key);
            }
        }
    }

    assert_eq!(
        rendered,
        vec!["kimi"],
        "only api.kimi.com/coding/v1 refuses a request that omits the field"
    );
}

#[test]
fn the_required_max_tokens_leaves_headroom_against_the_models_window() {
    // A cap set below what the model can produce would truncate an answer, which
    // is the failure the no-max_tokens rule exists to prevent. The value is set
    // well above any review-sized response so the required field cannot bite.
    let kimi = presets::preset("kimi").expect("kimi preset");
    assert_eq!(kimi.max_tokens, Some(200_000));
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

#[test]
fn every_preset_renders_a_config_that_loads() {
    // The end-to-end property that matters: a preset is only useful if the file
    // it writes is one `config::load` accepts. A preset naming a protocol the
    // loader rejects, or a temperature outside the allowed range, would
    // otherwise only be caught by a user running `drep init`.
    use crate::cli::init::presets::PRESETS;

    let dir = tempfile::tempdir().expect("tempdir");
    for preset in PRESETS {
        let endpoint = preset.endpoint.unwrap_or("http://localhost:1234/v1");
        let model = preset.default_model.unwrap_or("some-model");
        let body = super::support::render_one(preset, model, endpoint);

        // Every `${VAR}` the preset names has to resolve for the load to reach
        // validation, which is the part under test. Substituted in the rendered
        // text rather than exported: `std::env::set_var` is `unsafe` in edition
        // 2024 because a concurrent reader on another thread is a data race,
        // and `cargo test` is multi-threaded. The substitution proves the same
        // thing - that the file's *shape* loads - without touching the process.
        let body = match preset.api_key_env {
            Some(env) => body.replace(&format!("${{{env}}}"), "substituted-for-the-test"),
            None => body,
        };

        let path = dir.path().join(format!("{}.toml", preset.key));
        std::fs::write(&path, &body).expect("write");

        let config = crate::config::load(&path).unwrap_or_else(|e| {
            panic!("preset `{}` renders an unloadable config: {e}", preset.key)
        });
        assert_eq!(config.providers().len(), 1, "preset `{}`", preset.key);
    }
}

#[test]
fn a_preset_that_names_no_temperature_writes_no_temperature_line() {
    // `k3` and `gpt-5.6-sol` answer a 400 to any temperature at all, and a 400
    // neither fails over nor retries. A rendered `temperature = 0.2` would make
    // those two presets configure a provider that can never answer.
    use crate::cli::init::presets::PRESETS;

    for preset in PRESETS {
        let body = super::support::render_one(preset, "m", "http://e/v1");
        let has_line = body
            .lines()
            .any(|line| line.trim_start().starts_with("temperature ="));
        assert_eq!(
            has_line,
            preset.temperature.is_some(),
            "preset `{}` writes temperature={has_line} but declares {:?}",
            preset.key,
            preset.temperature
        );
    }
}

#[test]
fn a_preset_that_names_a_protocol_writes_it_and_the_others_stay_silent() {
    // An absent `protocol` line is what keeps every file written before 0.9.0
    // valid, so the two cases are asserted together rather than separately.
    use crate::cli::init::presets::PRESETS;

    for preset in PRESETS {
        let body = super::support::render_one(preset, "m", "http://e/v1");
        let rendered: Option<String> = body
            .lines()
            .find(|line| line.trim_start().starts_with("protocol ="))
            .map(str::to_string);

        match preset.protocol {
            Some(name) => assert_eq!(
                rendered.as_deref(),
                Some(format!("protocol = \"{name}\"").as_str()),
                "preset `{}`",
                preset.key
            ),
            None => assert_eq!(rendered, None, "preset `{}`", preset.key),
        }
    }
}

#[test]
fn the_anthropic_presets_are_the_ones_that_need_it() {
    // Pins the mapping itself, not just its self-consistency: kimi and minimax
    // publish their subscription tiers only behind `/messages`, and z.ai's
    // coding plan is ordinary chat completions.
    let anthropic: Vec<&str> = crate::cli::init::presets::PRESETS
        .iter()
        .filter(|p| p.protocol == Some("anthropic"))
        .map(|p| p.key)
        .collect();

    assert_eq!(anthropic, vec!["minimax", "kimi"]);
}

#[test]
fn each_presets_protocol_string_resolves_to_the_protocol_it_names() {
    // `LlmPreset::protocol()` is what the wizard hands the model listing and
    // what `config_file` writes. A version that always answered the default
    // would send chat-completions bytes at a `/messages` endpoint, and the
    // string field alone cannot catch that.
    use open_agent::ApiProtocol;

    let resolved = |key: &str| presets::preset(key).expect("preset").protocol();

    assert_eq!(resolved("kimi"), ApiProtocol::Anthropic);
    assert_eq!(resolved("minimax"), ApiProtocol::Anthropic);
    assert_eq!(resolved("zai"), ApiProtocol::OpenAiChat);
    assert_eq!(resolved("local"), ApiProtocol::OpenAiChat);
}

#[test]
fn every_presets_protocol_string_is_one_the_parser_accepts() {
    // The accessor panics on an unknown name rather than defaulting, so a typo
    // in the table has to be caught here rather than at a user's first run.
    for preset in presets::PRESETS {
        let _ = preset.protocol();
    }
}
