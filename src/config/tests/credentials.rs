//! The two credential fields, and the ambiguity between them.
//!
//! `api_key` and `api_key_command` answer the same question - where does this
//! provider's key come from - so a file that sets both has said two things. The
//! rule here is the one `UnknownProtocol` and `ZeroConcurrency` already follow:
//! reject, rather than pick a winner and let the user discover which.

use super::support::write_config;
use crate::config::{LlmConfig, load};

#[test]
fn an_api_key_command_round_trips_as_an_argv_array() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = write_config(
        &temp,
        r#"
[[llm]]
endpoint = "https://gateway.example/v1"
model = "m"
api_key_command = ["print-token", "--audience", "gateway"]
"#,
    );

    let provider = &load(&path).expect("a credential command loads").llm[0];
    assert_eq!(
        provider.api_key_command.as_deref(),
        Some(
            [
                "print-token".to_owned(),
                "--audience".to_owned(),
                "gateway".to_owned()
            ]
            .as_slice()
        ),
        "the argv is the field's whole value; dropping it makes the feature a \
         no-op that still loads"
    );
    assert_eq!(provider.api_key, None);
}

#[test]
fn an_api_key_and_an_api_key_command_on_one_entry_are_rejected() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = write_config(
        &temp,
        r#"
[[llm]]
endpoint = "https://gateway.example/v1"
model = "m"
api_key = "literal-for-test"
api_key_command = ["print-token"]
"#,
    );

    let message = load(&path)
        .expect_err("two answers to one question is not a precedence puzzle")
        .to_string();
    assert!(message.contains("#1 in file order"), "got {message}");
    assert!(message.contains("api_key"), "got {message}");
    assert!(message.contains("api_key_command"), "got {message}");
}

#[test]
fn an_empty_api_key_command_is_rejected_at_load() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = write_config(
        &temp,
        r#"
[[llm]]
endpoint = "https://gateway.example/v1"
model = "m"
api_key_command = []
"#,
    );

    let message = load(&path)
        .expect_err("an argv with no program names nothing to run")
        .to_string();
    assert!(message.contains("#1 in file order"), "got {message}");
    assert!(message.contains("api_key_command"), "got {message}");
}

#[test]
fn a_disabled_entry_may_declare_both_credential_fields() {
    // The parked-entry rule, which `${VAR}` expansion and field validation
    // already honour: a user switches an entry off precisely to stop it
    // mattering, and refusing to load the file over a provider that is never
    // contacted contradicts that where they would notice.
    let temp = tempfile::tempdir().expect("tempdir");
    let path = write_config(
        &temp,
        r#"
[[llm]]
enabled = false
endpoint = "https://gateway.example/v1"
model = "parked"
api_key = "literal-for-test"
api_key_command = []

[[llm]]
endpoint = "http://localhost:1234/v1"
model = "live"
"#,
    );

    let config = load(&path).expect("a parked entry is inert");
    assert_eq!(config.providers().len(), 1);
    assert_eq!(config.providers()[0].model.as_deref(), Some("live"));
}

#[test]
fn debug_of_a_key_command_prints_the_program_and_not_its_arguments() {
    // The argv is not the non-secret half of this field. A helper invoked as
    // `["vault", "read", "--token=…"]` carries the credential in its own
    // arguments, so the same reasoning that made `api_key` `<redacted>` applies
    // one field further along.
    let config = LlmConfig {
        api_key_command: Some(vec![
            "vault".to_owned(),
            "read".to_owned(),
            "--token=sekrit".to_owned(),
        ]),
        ..LlmConfig::default()
    };

    let rendered = format!("{config:?}");
    assert!(
        rendered.contains("vault"),
        "the program name is the useful half: {rendered}"
    );
    assert!(
        !rendered.contains("sekrit"),
        "an argument reached a log: {rendered}"
    );
    assert!(
        !rendered.contains("--token"),
        "no argument is printed, not just the ones that look secret: {rendered}"
    );
}
