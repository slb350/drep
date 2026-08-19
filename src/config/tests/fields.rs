//! Field parsing, defaults and validation.
//!
//! Every field has a documented default so a partial file works; the two that
//! carry real consequence are `max_tokens` (absent means *no cap is sent*, not
//! zero) and `temperature` (range-checked, because serde cannot).

use super::support::write_config;
use crate::config::*;
use std::path::PathBuf;

#[test]
fn full_toml_round_trips_into_config_with_every_field_correct() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = write_config(
        &temp,
        r#"
[[llm]]
enabled = true
endpoint = "http://localhost:11434/v1"
model = "qwen3:8b"
api_key = "literal-secret"
temperature = 0.7
max_tokens = 4096
timeout_secs = 120
max_retries = 5
max_concurrent = 8
"#,
    );

    let config = load(&path).expect("load");
    let llm = &config.llm[0];
    assert!(llm.enabled);
    assert_eq!(llm.endpoint.as_deref(), Some("http://localhost:11434/v1"));
    assert_eq!(llm.model.as_deref(), Some("qwen3:8b"));
    assert_eq!(llm.api_key.as_deref(), Some("literal-secret"));
    assert!((llm.temperature - 0.7).abs() < f32::EPSILON);
    assert_eq!(llm.max_tokens, Some(4096));
    assert_eq!(llm.timeout_secs, 120);
    assert_eq!(llm.max_retries, 5);
    assert_eq!(llm.max_concurrent, 8);
}

#[test]
fn partial_file_uses_documented_defaults_and_max_tokens_is_none() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = write_config(
        &temp,
        r#"
[[llm]]
enabled = true
model = "qwen3:8b"
"#,
    );

    let config = load(&path).expect("load");
    let llm = &config.llm[0];
    assert!(llm.enabled);
    assert_eq!(llm.model.as_deref(), Some("qwen3:8b"));
    assert!(llm.endpoint.is_none());
    assert!(llm.api_key.is_none());
    assert!((llm.temperature - 0.2).abs() < f32::EPSILON, "default 0.2");
    assert_eq!(llm.max_tokens, None, "absent max_tokens is None, not 0");
    assert_eq!(llm.timeout_secs, 60, "default timeout");
    assert_eq!(llm.max_retries, 3, "default max_retries");
    assert_eq!(llm.max_concurrent, 3, "default max_concurrent");
}

#[test]
fn temperature_outside_range_is_rejected() {
    for bad in ["-0.1", "2.5", "100.0"] {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = write_config(
            &temp,
            &format!(
                r#"
[[llm]]
temperature = {bad}
"#
            ),
        );
        let err = load(&path).expect_err("should reject");
        assert!(
            matches!(err, ConfigError::Temperature { .. }),
            "expected Temperature error, got {err:?} for value {bad}"
        );
    }
}

#[test]
fn missing_file_path_is_an_error() {
    let temp = tempfile::tempdir().expect("tempdir");
    let missing = temp.path().join("does_not_exist.toml");
    let err = load(&missing).expect_err("missing file must error");
    assert!(
        matches!(err, ConfigError::Io(_, _)),
        "expected Io error, got {err:?}"
    );
}

#[test]
fn max_tokens_absent_yields_none_and_present_yields_some() {
    let temp = tempfile::tempdir().expect("tempdir");
    let absent = write_config(&temp, "[[llm]]\nmodel = \"x\"\n");
    let config = load(&absent).expect("load");
    assert_eq!(config.llm[0].max_tokens, None);

    let present = write_config(&temp, "[[llm]]\nmax_tokens = 8192\n");
    let config = load(&present).expect("load");
    assert_eq!(config.llm[0].max_tokens, Some(8192));
}

/// A file with no `[[llm]]` at all is rejected, not defaulted.
///
/// The LLM layer is mandatory in 2.x, so a config naming no provider can
/// never produce a passing run. Tolerating it here would push the failure
/// down to `LlmClient::new`, which reports "not configured" without saying
/// which file is at fault.

#[test]
fn default_config_path_is_drep_toml_in_cwd() {
    assert_eq!(default_config_path(), PathBuf::from("drep.toml"));
}

/// A `Debug`-printed config redacts the API key.
///
/// `Config` derives `Debug` and holds these, so any `{:?}`, `dbg!` or tracing
/// line touching a loaded config would otherwise emit a live credential.
/// `LlmClient` already hand-writes `Debug` for exactly this reason - the config
/// it is *built from* held the same secret in the clear.
#[test]
fn debug_redacts_the_api_key() {
    let cfg = LlmConfig {
        api_key: Some("sk-a-real-looking-secret".to_owned()),
        model: Some("m".to_owned()),
        ..LlmConfig::default()
    };

    let printed = format!("{cfg:?}");
    assert!(
        !printed.contains("sk-a-real-looking-secret"),
        "the key must never reach a log: {printed}"
    );
    assert!(
        printed.contains("<redacted>"),
        "and its presence must still be visible: {printed}"
    );
    // The rest of the struct is still useful for debugging.
    assert!(printed.contains("\"m\""), "got {printed}");

    let absent = LlmConfig::default();
    assert!(
        format!("{absent:?}").contains("None"),
        "an absent key reads as absent, not as redacted"
    );
}
