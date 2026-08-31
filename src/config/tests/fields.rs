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
max_review_rounds = 7

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
headers = { "X-Trace" = "on" }
"#,
    );

    let config = load(&path).expect("load");
    assert_eq!(config.max_review_rounds, 7);
    let llm = &config.llm[0];
    assert!(llm.enabled);
    assert_eq!(llm.endpoint.as_deref(), Some("http://localhost:11434/v1"));
    assert_eq!(llm.model.as_deref(), Some("qwen3:8b"));
    assert_eq!(llm.api_key.as_deref(), Some("literal-secret"));
    assert!(
        llm.temperature
            .is_some_and(|t| (t - 0.7).abs() < f32::EPSILON),
        "an explicit temperature survives the round trip, got {:?}",
        llm.temperature
    );
    assert_eq!(llm.max_tokens, Some(4096));
    assert_eq!(llm.timeout_secs, 120);
    assert_eq!(llm.max_retries, 5);
    assert_eq!(llm.max_concurrent, 8);
    assert_eq!(llm.headers.get("X-Trace").map(String::as_str), Some("on"));
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
    assert_eq!(
        config.max_review_rounds, DEFAULT_MAX_REVIEW_ROUNDS,
        "default semantic review budget"
    );
    let llm = &config.llm[0];
    assert!(llm.enabled);
    assert_eq!(llm.model.as_deref(), Some("qwen3:8b"));
    assert!(llm.endpoint.is_none());
    assert!(llm.api_key.is_none());
    assert_eq!(
        llm.temperature, None,
        "an absent temperature is None, not a default value: the parameter is then \
         omitted from the request entirely, which is the only thing that works against a \
         model that rejects it"
    );
    assert_eq!(
        llm.protocol, None,
        "absent protocol means the default, openai"
    );
    assert_eq!(llm.max_tokens, None, "absent max_tokens is None, not 0");
    assert_eq!(llm.timeout_secs, 60, "default timeout");
    assert_eq!(llm.max_retries, 3, "default max_retries");
    assert_eq!(llm.max_concurrent, 3, "default max_concurrent");
}

#[test]
fn zero_max_review_rounds_is_rejected() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = write_config(&temp, "max_review_rounds = 0\n\n[[llm]]\nmodel = \"m\"\n");

    let err = load(&path).expect_err("zero cannot bound a review loop");
    assert!(
        matches!(err, ConfigError::ZeroReviewRounds),
        "expected ZeroReviewRounds, got {err:?}"
    );
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

#[test]
fn zero_timeout_and_zero_output_budget_are_rejected() {
    for (field, expected) in [
        ("timeout_secs = 0", "timeout_secs"),
        ("max_tokens = 0", "max_tokens"),
    ] {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = write_config(&temp, &format!("[[llm]]\n{field}\n"));
        let err = load(&path).expect_err("a review needs time and output capacity");
        assert!(err.to_string().contains(expected), "got {err:?}");
    }
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

#[test]
fn a_protocol_name_is_read_and_parsed() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = write_config(
        &temp,
        r#"
[[llm]]
model = "k3"
endpoint = "https://api.kimi.com/coding/v1"
protocol = "anthropic"
"#,
    );

    let config = load(&path).expect("load");
    assert_eq!(config.llm[0].protocol.as_deref(), Some("anthropic"));
    assert_eq!(
        crate::config::parse_protocol(config.llm[0].protocol.as_deref()),
        Some(open_agent::ApiProtocol::Anthropic)
    );
}

#[test]
fn an_absent_protocol_parses_as_the_default_rather_than_failing() {
    // What keeps every file written before this feature valid. A `None` return
    // here would make an unannotated provider unloadable.
    assert_eq!(
        crate::config::parse_protocol(None),
        Some(open_agent::ApiProtocol::OpenAiChat)
    );
}

#[test]
fn a_protocol_name_is_matched_case_insensitively() {
    assert_eq!(
        crate::config::parse_protocol(Some("Anthropic")),
        Some(open_agent::ApiProtocol::Anthropic)
    );
}

#[test]
fn an_unknown_protocol_is_rejected_by_name_and_position() {
    // Defaulting instead would post chat-completions bytes to a `/messages`
    // endpoint, and the 404 reads as the provider being down rather than as a
    // typo on this line.
    let temp = tempfile::tempdir().expect("tempdir");
    let path = write_config(
        &temp,
        r#"
[[llm]]
model = "a"
endpoint = "http://a/v1"

[[llm]]
model = "b"
endpoint = "http://b/v1"
protocol = "antropic"
"#,
    );

    let err = load(&path).expect_err("an unknown protocol is fatal");
    let message = err.to_string();
    assert!(message.contains("antropic"), "names the value: {message}");
    assert!(message.contains("#2"), "names the position: {message}");
}

#[test]
fn a_disabled_entrys_unknown_protocol_does_not_refuse_the_file() {
    // `enabled = false` means the entry is inert. Refusing to load because a
    // parked provider has a typo contradicts that in the one place a user would
    // notice - they parked it precisely to stop it mattering.
    let temp = tempfile::tempdir().expect("tempdir");
    let path = write_config(
        &temp,
        r#"
[[llm]]
enabled = false
model = "parked"
endpoint = "http://a/v1"
protocol = "nonsense"

[[llm]]
model = "live"
endpoint = "http://b/v1"
"#,
    );

    let config = load(&path).expect("a parked entry is inert");
    assert_eq!(config.providers().len(), 1);
}

#[test]
fn refuse_markers_in_drep_toml_is_rejected_rather_than_ignored() {
    // The worst failure available here. `Config` has no such field, so serde
    // would drop the key without a word: a developer reads their own config,
    // believes the repository is protected, and every review still ships its
    // source. Rejecting it is also what tells them where the field does belong.
    let temp = tempfile::tempdir().expect("tempdir");
    let path = write_config(
        &temp,
        r#"
refuse_markers = [".drep-no-llm"]

[[llm]]
model = "a"
endpoint = "http://a/v1"
"#,
    );

    let err = load(&path).expect_err("a repository cannot declare site policy");
    let message = err.to_string();
    assert!(
        message.contains("refuse_markers"),
        "names the field: {message}"
    );
    assert!(
        message.contains("site policy"),
        "and says where it belongs: {message}"
    );
    assert!(
        message.contains(&path.display().to_string()),
        "and which file it was read from: {message}"
    );
    assert!(
        message.contains(&crate::config::site::machine_path().display().to_string()),
        "and where it does belong, so the reader can act on it rather than being \
         told only where it does not: {message}"
    );
}

#[test]
fn max_concurrent_ceiling_in_drep_toml_is_rejected_rather_than_ignored() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = write_config(
        &temp,
        r#"
max_concurrent_ceiling = 2

[[llm]]
model = "a"
endpoint = "http://a/v1"
"#,
    );

    let err = load(&path).expect_err("a repository cannot declare a site ceiling");
    let message = err.to_string();
    assert!(message.contains("max_concurrent_ceiling"), "got {message}");
    assert!(message.contains("site policy"), "got {message}");
}
