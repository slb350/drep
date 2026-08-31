//! Backend discrimination and backend-specific field validation.

use super::support::write_config;
use crate::config::{BackendKind, ReasoningEffort, load};

#[test]
fn backend_wire_names_are_stable() {
    assert_eq!(BackendKind::Http.as_str(), "http");
    assert_eq!(BackendKind::Codex.as_str(), "codex");
}

#[test]
fn an_old_config_without_backend_is_http_unchanged() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = write_config(
        &temp,
        r#"
[[llm]]
endpoint = "https://api.openai.com/v1"
model = "gpt-5.6-sol"
api_key = "literal-for-test"
"#,
    );

    let config = load(&path).expect("old config still loads");
    let provider = &config.llm[0];
    assert_eq!(provider.backend, BackendKind::Http);
    assert_eq!(provider.reasoning_effort, None);
    assert_eq!(
        provider.endpoint.as_deref(),
        Some("https://api.openai.com/v1")
    );
}

#[test]
fn codex_backend_loads_as_typed_subscription_configuration() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = write_config(
        &temp,
        r#"
[[llm]]
backend = "codex"
model = "gpt-5.6-sol"
reasoning_effort = "high"
timeout_secs = 1800
max_concurrent = 1
"#,
    );

    let provider = &load(&path).expect("codex config loads").llm[0];
    assert_eq!(provider.backend, BackendKind::Codex);
    assert_eq!(provider.reasoning_effort, Some(ReasoningEffort::High));
    assert_eq!(provider.model.as_deref(), Some("gpt-5.6-sol"));
    assert_eq!(provider.timeout_secs, 1800);
    assert_eq!(provider.max_concurrent, 1);
}

#[test]
fn every_documented_codex_reasoning_effort_deserializes_exactly() {
    for (wire, expected) in [
        ("minimal", ReasoningEffort::Minimal),
        ("low", ReasoningEffort::Low),
        ("medium", ReasoningEffort::Medium),
        ("high", ReasoningEffort::High),
        ("xhigh", ReasoningEffort::Xhigh),
    ] {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = write_config(
            &temp,
            &format!(
                "[[llm]]\nbackend = \"codex\"\nmodel = \"gpt-5.6-sol\"\nreasoning_effort = \"{wire}\"\n"
            ),
        );

        let provider = &load(&path).expect("documented effort loads").llm[0];
        assert_eq!(provider.reasoning_effort, Some(expected.clone()));
        assert_eq!(expected.as_str(), wire);
    }
}

#[test]
fn unknown_backend_and_reasoning_effort_name_the_table_in_file_order() {
    for (field, value) in [("backend", "socket"), ("reasoning_effort", "huge")] {
        let temp = tempfile::tempdir().expect("tempdir");
        let backend = if field != "backend" {
            "backend = \"codex\""
        } else {
            ""
        };
        let path = write_config(
            &temp,
            &format!(
                r#"
[[llm]]
model = "first"

[[llm]]
{backend}
model = "second"
{field} = "{value}"
"#
            ),
        );

        let message = load(&path)
            .expect_err("unknown value is rejected")
            .to_string();
        assert!(message.contains("#2 in file order"), "got {message}");
        assert!(message.contains(value), "got {message}");
    }
}

#[test]
fn codex_rejects_every_explicit_http_only_field() {
    let cases = [
        ("endpoint", "endpoint = \"https://api.openai.com/v1\""),
        ("api_key", "api_key = \"literal-for-test\""),
        ("api_key_command", "api_key_command = [\"print-token\"]"),
        ("headers", "headers = { \"X-Anything\" = \"v\" }"),
        ("protocol", "protocol = \"openai\""),
        ("temperature", "temperature = 0.2"),
        ("max_tokens", "max_tokens = 4096"),
        ("max_retries", "max_retries = 2"),
    ];

    for (field, line) in cases {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = write_config(
            &temp,
            &format!("[[llm]]\nbackend = \"codex\"\nmodel = \"gpt-5.6-sol\"\n{line}\n"),
        );

        let message = load(&path)
            .expect_err("HTTP field must not be ignored")
            .to_string();
        assert!(message.contains("#1 in file order"), "got {message}");
        assert!(message.contains("codex"), "got {message}");
        assert!(message.contains(field), "got {message}");
    }
}

#[test]
fn http_rejects_codex_only_reasoning_effort() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = write_config(
        &temp,
        r#"
[[llm]]
backend = "http"
endpoint = "http://localhost:1234/v1"
model = "m"
reasoning_effort = "high"
"#,
    );

    let message = load(&path)
        .expect_err("Codex-only field must not be ignored")
        .to_string();
    assert!(message.contains("reasoning_effort"), "got {message}");
    assert!(message.contains("http"), "got {message}");
}

#[test]
fn codex_requires_a_model_at_config_load() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = write_config(&temp, "[[llm]]\nbackend = \"codex\"\n");

    let message = load(&path).expect_err("Codex needs a model").to_string();
    assert!(message.contains("#1 in file order"), "got {message}");
    assert!(message.contains("model"), "got {message}");
}

#[test]
fn codex_rejects_a_whitespace_only_model_at_config_load() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = write_config(&temp, "[[llm]]\nbackend = \"codex\"\nmodel = \"  \"\n");

    assert!(
        load(&path)
            .expect_err("Codex needs a usable model")
            .to_string()
            .contains("model")
    );
}

#[test]
fn codex_reasoning_effort_is_optional_for_a_hand_written_config() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = write_config(
        &temp,
        "[[llm]]\nbackend = \"codex\"\nmodel = \"gpt-5.6-sol\"\n",
    );

    let provider = &load(&path)
        .expect("Codex may use the CLI default effort")
        .llm[0];
    assert_eq!(provider.reasoning_effort, None);
}

#[test]
fn a_disabled_backend_entry_is_fully_inert() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = write_config(
        &temp,
        r#"
[[llm]]
enabled = false
backend = "not-a-backend"
reasoning_effort = "not-an-effort"
endpoint = "https://unused.example/v1"
api_key = "${DREP_TEST_VAR_THAT_IS_NOT_SET}"
protocol = "not-a-protocol"
temperature = 9.0
max_retries = 0
max_concurrent = 0

[[llm]]
model = "live"
endpoint = "http://localhost:1234/v1"
"#,
    );

    let config = load(&path).expect("disabled entry is inert");
    assert_eq!(config.providers().len(), 1);
    assert_eq!(config.providers()[0].backend, BackendKind::Http);
}
