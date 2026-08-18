//! B3, B4, B5, B6, B7: config_file rendering and writing.

use crate::cli::init::config_file;
use crate::cli::init::presets;

#[test]
fn openrouter_render_parses_and_carries_literal_api_key_reference() {
    let preset = presets::preset("openrouter").expect("openrouter");
    let body = config_file::render(preset, "m", "http://e/v1");

    let value: toml::Value = toml::from_str(&body).expect("renders as valid TOML");
    let entries = value
        .get("llm")
        .and_then(|v| v.as_array())
        .expect("[[llm]] is an array");
    assert_eq!(entries.len(), 1, "exactly one [[llm]] entry");
    let entry = &entries[0];
    assert_eq!(
        entry.get("api_key").and_then(|v| v.as_str()),
        Some("${OPENROUTER_API_KEY}"),
        "the api_key is the literal variable name, not an expansion"
    );
}

#[test]
fn local_render_omits_api_key_and_timeout_secs_but_openrouter_has_both() {
    let local = presets::preset("local").expect("local");
    let local_body = config_file::render(local, "m", "http://e/v1");
    let local_value: toml::Value = toml::from_str(&local_body).expect("local parses");
    let local_entry = &local_value
        .get("llm")
        .and_then(|v| v.as_array())
        .expect("array")[0];
    assert!(
        local_entry.get("api_key").is_none(),
        "local preset does not write api_key, got {local_body:?}"
    );
    assert!(
        local_entry.get("timeout_secs").is_none(),
        "local preset does not write timeout_secs, got {local_body:?}"
    );

    let openrouter = presets::preset("openrouter").expect("openrouter");
    let or_body = config_file::render(openrouter, "m", "http://e/v1");
    let or_value: toml::Value = toml::from_str(&or_body).expect("openrouter parses");
    let or_entry = &or_value
        .get("llm")
        .and_then(|v| v.as_array())
        .expect("array")[0];
    assert!(
        or_entry.get("api_key").is_some(),
        "openrouter preset writes api_key, got {or_body:?}"
    );
    assert!(
        or_entry.get("timeout_secs").is_some(),
        "openrouter preset writes timeout_secs, got {or_body:?}"
    );
}

#[test]
fn rendered_config_loads_through_config_load_when_env_var_is_set() {
    let preset = presets::preset("openrouter").expect("openrouter");
    let body = config_file::render(preset, "m", "http://e/v1");
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("drep.toml");
    std::fs::write(&path, body).expect("write");

    // The variable is required to be set during the test; `config::load`
    // expands `${VAR}` and reports unset variables. Restore afterwards so a
    // later test does not see a value that drep itself never set.
    //
    // SAFETY: `set_var` and `remove_var` are `unsafe` in edition 2024 because
    // any other thread reading the environment concurrently is a data race.
    // No other test reads or writes `OPENROUTER_API_KEY`, so the window is
    // closed.
    unsafe {
        std::env::set_var("OPENROUTER_API_KEY", "sk-test-key");
    }
    let loaded = crate::config::load(&path).expect("config loads");
    unsafe {
        std::env::remove_var("OPENROUTER_API_KEY");
    }

    assert_eq!(loaded.llm.len(), 1);
    let primary = *loaded.providers().first().expect("a provider is written");
    assert_eq!(primary.model.as_deref(), Some("m"));
}

#[test]
fn render_escapes_quote_and_backslash_in_model_names() {
    let preset = presets::preset("local").expect("local");
    let nasty = "a\"b\\c";
    let body = config_file::render(preset, nasty, "http://e/v1");
    let value: toml::Value = toml::from_str(&body).expect("still parses as TOML");
    let entry = &value.get("llm").and_then(|v| v.as_array()).expect("array")[0];
    assert_eq!(
        entry.get("model").and_then(|v| v.as_str()),
        Some(nasty),
        "round-tripped model equals input"
    );
}

#[test]
fn write_refuses_to_overwrite_existing_drep_toml_without_force() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("drep.toml");
    let original = "original contents\n";
    std::fs::write(&path, original).expect("seed");

    let result = config_file::write(dir.path(), "new body\n", false);
    let err = result.expect_err("refuses to overwrite");
    let msg = format!("{err:#}");
    assert!(
        msg.contains(path.display().to_string().as_str()),
        "error names the path; got: {msg}"
    );
    assert!(
        msg.contains("--force"),
        "error mentions --force; got: {msg}"
    );
    let unchanged = std::fs::read_to_string(&path).expect("read");
    assert_eq!(unchanged, original, "file contents unchanged on refusal");
}

#[test]
fn write_with_force_replaces_contents() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("drep.toml");
    std::fs::write(&path, "original\n").expect("seed");

    let returned = config_file::write(dir.path(), "new body\n", true).expect("force ok");
    assert_eq!(returned, path);
    let after = std::fs::read_to_string(&path).expect("read");
    assert_eq!(after, "new body\n");
}

/// Every rendered config parses as TOML, whatever is in the model or the
/// endpoint.
///
/// The property is "init never writes a file config cannot read", and the
/// characters that break it are not the obvious ones: a literal `\r` or `\n`
/// inside a TOML basic string is a parse error, and a URL pasted out of a CRLF
/// file carries one. `drep init` then reported success and left a `drep.toml`
/// that every later `drep check` and `drep doctor` refused to load - and that
/// `write` would not replace without `--force`.
///
/// The endpoint is varied as well as the model: `render` escapes both, and a
/// test that only varied the model passed with the endpoint's `escape` call
/// deleted.
#[test]
fn a_rendered_config_always_parses_whatever_the_model_and_endpoint_contain() {
    let preset = crate::cli::init::presets::preset("openrouter").expect("preset");
    let nasty = [
        "plain",
        "with \"quote\"",
        "with\\backslash",
        "with\nnewline",
        "with\rcarriage",
        "with\ttab",
        "with\u{7f}delete",
        "with\u{1}control",
        "unicode-\u{e9}\u{4e2d}",
    ];

    for value in nasty {
        for (model, endpoint) in [(value, "http://h/v1"), ("m", value)] {
            let body = crate::cli::init::config_file::render(preset, model, endpoint);
            let parsed: toml::Value = toml::from_str(&body)
                .unwrap_or_else(|e| panic!("render({model:?}, {endpoint:?}) is not TOML: {e}"));

            let entry = &parsed["llm"].as_array().expect("array")[0];
            assert_eq!(
                entry["model"].as_str(),
                Some(model),
                "the model must round-trip exactly"
            );
            assert_eq!(
                entry["endpoint"].as_str(),
                Some(endpoint),
                "and so must the endpoint"
            );
        }
    }
}
