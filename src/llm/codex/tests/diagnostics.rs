//! Only the version and ChatGPT-auth facts survive diagnostic parsing.

use crate::llm::codex::command::ChildEnvironment;
use crate::llm::codex::diagnostics::{DiagnosticError, parse, poll_before_deadline, probe};

const CHATGPT: &str = r#"{
  "schemaVersion": 1,
  "codexVersion": "0.148.0",
  "checks": {
    "auth.credentials": {
      "details": {
        "auth file": "/private/account/path/auth.json",
        "stored API key": "false",
        "stored ChatGPT tokens": "true",
        "stored auth mode": "chatgpt"
      }
    }
  }
}"#;

#[test]
fn parser_retains_only_version_and_chatgpt_auth_state() {
    let diagnostic = parse(CHATGPT.as_bytes()).expect("known diagnostic");
    assert_eq!(diagnostic.cli_version(), "0.148.0");
    assert_eq!(
        format!("{diagnostic:?}"),
        "CodexStatus { cli_version: \"0.148.0\" }"
    );
    assert!(!format!("{diagnostic:?}").contains("/private/account"));
}

#[test]
fn api_auth_is_rejected_without_echoing_diagnostic_values() {
    let api = CHATGPT
        .replace(
            "\"stored API key\": \"false\"",
            "\"stored API key\": \"true\"",
        )
        .replace(
            "\"stored ChatGPT tokens\": \"true\"",
            "\"stored ChatGPT tokens\": \"false\"",
        )
        .replace(
            "\"stored auth mode\": \"chatgpt\"",
            "\"stored auth mode\": \"api\"",
        );
    let err = parse(api.as_bytes()).expect_err("API auth cannot fund subscription usage");
    assert!(matches!(err, DiagnosticError::NotChatGpt));
    assert!(!err.to_string().contains("api"));
}

#[test]
fn chatgpt_auth_is_accepted_even_when_an_api_key_is_also_stored() {
    let both = CHATGPT.replace(
        "\"stored API key\": \"false\"",
        "\"stored API key\": \"true\"",
    );

    assert!(
        parse(both.as_bytes()).is_ok(),
        "forced_login_method=chatgpt, not the absence of an unrelated key, is the runtime guarantee"
    );
}

#[test]
fn a_missing_binary_has_actionable_redacted_guidance() {
    let err = probe(
        std::path::Path::new("/definitely-not-a-real-drep-codex-binary"),
        &ChildEnvironment::default(),
    )
    .expect_err("binary is absent");

    assert!(matches!(err, DiagnosticError::MissingBinary));
    assert!(err.to_string().contains("`codex login`"));
}

#[test]
#[cfg(unix)]
fn valid_auth_diagnostic_survives_an_unrelated_nonzero_doctor_status() {
    let dir = tempfile::tempdir().expect("tempdir");
    let executable = dir.path().join("fake-codex");
    crate::test_support::write_executable(
        &executable,
        format!("#!/bin/sh\nprintf '%s' '{}'\nexit 1\n", CHATGPT),
    );

    let diagnostic = probe(&executable, &ChildEnvironment::default())
        .expect("known redacted auth facts are usable despite an unrelated doctor warning");
    assert_eq!(diagnostic.cli_version(), "0.148.0");
}

#[test]
#[cfg(unix)]
fn diagnostic_output_accepts_exactly_one_mebibyte() {
    const EXPECTED_LIMIT: usize = 1024 * 1024;
    let dir = tempfile::tempdir().expect("tempdir");
    let executable = diagnostic_executable(dir.path(), diagnostic_with_len(EXPECTED_LIMIT));

    let diagnostic = probe(&executable, &ChildEnvironment::default())
        .expect("the exact diagnostic ceiling is accepted");
    assert_eq!(diagnostic.cli_version(), "0.148.0");
}

#[test]
#[cfg(unix)]
fn diagnostic_output_one_byte_over_the_limit_is_rejected_as_too_large() {
    const EXPECTED_LIMIT: usize = 1024 * 1024;
    let dir = tempfile::tempdir().expect("tempdir");
    let executable = diagnostic_executable(dir.path(), diagnostic_with_len(EXPECTED_LIMIT + 1));

    assert!(matches!(
        probe(&executable, &ChildEnvironment::default()),
        Err(DiagnosticError::OutputTooLarge)
    ));
}

#[test]
#[cfg(unix)]
fn an_empty_nonzero_diagnostic_is_a_process_failure() {
    let dir = tempfile::tempdir().expect("tempdir");
    let executable = dir.path().join("fake-codex");
    crate::test_support::write_executable(&executable, "#!/bin/sh\nexit 1\n");

    assert!(matches!(
        probe(&executable, &ChildEnvironment::default()),
        Err(DiagnosticError::Process)
    ));
}

#[cfg(unix)]
#[test]
fn a_grandchild_inheriting_stdout_cannot_hold_the_diagnostic_open() {
    let dir = tempfile::tempdir().expect("tempdir");
    let executable = dir.path().join("fake-codex");
    crate::test_support::write_executable(
        &executable,
        format!(
            "#!/bin/sh\ncapture=$(dirname \"$0\")\nsleep 4 &\nprintf '%s' \"$!\" > \"$capture/grandchild.pid\"\nprintf '%s' '{}'\n",
            CHATGPT
        ),
    );

    let status = probe(&executable, &ChildEnvironment::default()).expect("valid diagnostic");
    let pid = std::fs::read_to_string(dir.path().join("grandchild.pid")).expect("grandchild pid");
    let running = super::probe_and_stop_process(&pid);

    assert_eq!(status.cli_version(), "0.148.0");
    assert!(running, "probe waited for the unrelated grandchild to exit");
}

#[test]
fn chatgpt_tokens_and_chatgpt_mode_are_both_required() {
    let wrong_mode = CHATGPT.replace(
        "\"stored auth mode\": \"chatgpt\"",
        "\"stored auth mode\": \"api\"",
    );
    let no_tokens = CHATGPT.replace(
        "\"stored ChatGPT tokens\": \"true\"",
        "\"stored ChatGPT tokens\": \"false\"",
    );

    for document in [wrong_mode, no_tokens] {
        assert!(matches!(
            parse(document.as_bytes()),
            Err(DiagnosticError::NotChatGpt)
        ));
    }
}

#[test]
fn polling_stops_at_the_deadline_not_after_it() {
    let deadline = std::time::Duration::from_secs(30);
    assert!(poll_before_deadline(
        std::time::Duration::from_secs(29),
        deadline
    ));
    assert!(!poll_before_deadline(deadline, deadline));
}

#[test]
fn unknown_schema_fails_closed() {
    let future = CHATGPT.replace("\"schemaVersion\": 1", "\"schemaVersion\": 2");
    assert!(matches!(
        parse(future.as_bytes()),
        Err(DiagnosticError::UnsupportedFormat)
    ));
}

fn diagnostic_with_len(length: usize) -> Vec<u8> {
    let mut value: serde_json::Value = serde_json::from_str(CHATGPT).expect("fixture JSON");
    value["padding"] = serde_json::Value::String(String::new());
    let fixed = serde_json::to_vec(&value).expect("fixture serializes");
    let padding = length
        .checked_sub(fixed.len())
        .expect("requested size fits");
    value["padding"] = serde_json::Value::String("x".repeat(padding));
    let bytes = serde_json::to_vec(&value).expect("padded fixture serializes");
    assert_eq!(bytes.len(), length);
    bytes
}

#[cfg(unix)]
fn diagnostic_executable(dir: &std::path::Path, bytes: Vec<u8>) -> std::path::PathBuf {
    let executable = dir.join("fake-codex");
    std::fs::write(dir.join("diagnostic.json"), bytes).expect("diagnostic fixture");
    crate::test_support::write_executable(
        &executable,
        "#!/bin/sh\ncapture=$(dirname \"$0\")\nexec /bin/cat \"$capture/diagnostic.json\"\n",
    );
    executable
}
