//! A fake executable proves the real process receives only the intended surface.

use std::ffi::OsString;
use std::time::Duration;

use serde_json::json;

use crate::config::{BackendKind, LlmConfig, ReasoningEffort};
use crate::llm::codex::CodexClient;
use crate::llm::codex::command::ChildEnvironment;
use crate::llm::codex::process;
use crate::llm::error::{BackendErrorKind, LlmError};
use crate::llm::json_parsing::Extracted;

#[test]
fn client_accessors_and_identity_preserve_the_configured_contract() {
    let (dir, client) = fake_client("#!/bin/sh\nexit 0\n", 5);

    assert_eq!(client.model(), "gpt-5.6-sol");
    assert_eq!(client.cli_version(), "0.148.0");
    assert_eq!(client.reasoning_effort(), Some(&ReasoningEffort::High));
    assert_eq!(client.identity(), "codex:chatgpt:cli=0.148.0:effort=high");
    drop(dir);
}

#[tokio::test]
async fn fake_codex_receives_stdin_empty_cwd_and_allowlisted_environment() {
    let dir = tempfile::tempdir().expect("tempdir");
    let executable = dir.path().join("fake-codex");
    crate::test_support::write_executable(
        &executable,
        r#"#!/bin/sh
set -eu
capture=$(dirname "$0")
printf '%s\n' "$@" > "$capture/args"
pwd > "$capture/cwd"
ls -A > "$capture/cwd_entries"
env | sort > "$capture/env"
sed -n '1,$p' > "$capture/stdin"
printf '%s\n' \
  '{"type":"thread.started","thread_id":"redacted"}' \
  '{"type":"turn.started"}' \
  '{"type":"item.completed","item":{"type":"agent_message","text":"{\"issues\":[],\"summary\":\"clean\"}"}}' \
  '{"type":"turn.completed","usage":{"input_tokens":12,"output_tokens":4}}'
"#,
    );

    let cfg = LlmConfig {
        backend: BackendKind::Codex,
        model: Some("gpt-5.6-sol".to_owned()),
        reasoning_effort: Some(ReasoningEffort::High),
        timeout_secs: 5,
        max_concurrent: 1,
        ..LlmConfig::default()
    };
    let environment = ChildEnvironment::from_iter([
        (OsString::from("PATH"), OsString::from("/usr/bin:/bin")),
        (OsString::from("HOME"), OsString::from("/safe/home")),
        (
            OsString::from("CODEX_HOME"),
            OsString::from("/safe/home/.codex"),
        ),
        (
            OsString::from("OPENAI_API_KEY"),
            OsString::from("must-not-leak"),
        ),
        (
            OsString::from("DREP_AUTH_PATH"),
            OsString::from("/real/auth.toml"),
        ),
    ]);
    let client =
        CodexClient::at(&cfg, executable, environment, "0.148.0").expect("valid test client");

    let result = client
        .complete_json("Review Rust carefully.", "diff --git a/a.rs b/a.rs")
        .await
        .expect("fake Codex succeeds");
    assert_eq!(
        result,
        Extracted::Complete(json!({"issues": [], "summary": "clean"}))
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("stdin")).expect("stdin capture"),
        "diff --git a/a.rs b/a.rs"
    );
    let cwd = std::fs::read_to_string(dir.path().join("cwd")).expect("cwd capture");
    assert_ne!(cwd.trim(), env!("CARGO_MANIFEST_DIR"));
    assert_eq!(
        std::fs::read_to_string(dir.path().join("cwd_entries")).expect("cwd entries"),
        ""
    );

    let environment = std::fs::read_to_string(dir.path().join("env")).expect("env capture");
    assert!(environment.contains("HOME=/safe/home\n"));
    assert!(environment.contains("CODEX_HOME=/safe/home/.codex\n"));
    assert!(!environment.contains("OPENAI_API_KEY"));
    assert!(!environment.contains("DREP_AUTH_PATH"));

    let args = std::fs::read_to_string(dir.path().join("args")).expect("args capture");
    assert!(args.contains("forced_login_method=\"chatgpt\"\n"));
    assert!(args.contains("--ignore-user-config\n"));
    assert!(args.contains("--ephemeral\n"));
    assert!(args.ends_with("--json\n-\n"));
}

#[tokio::test]
async fn forbidden_tool_activity_is_a_sticky_contract_failure() {
    let (dir, client) = fake_client(
        concat!(
            "#!/bin/sh\n",
            "sed -n '1,$p' >/dev/null\n",
            "printf '%s\\n' \\\n",
            "  '{\"type\":\"item.completed\",\"item\":{\"type\":\"web_search\"}}' \\\n",
            "  '{\"type\":\"item.completed\",\"item\":{\"type\":\"agent_message\",\"text\":\"{\\\"issues\\\":[],\\\"summary\\\":\\\"clean\\\"}\"}}' \\\n",
            "  '{\"type\":\"turn.completed\"}'\n",
        ),
        5,
    );
    let err = client
        .complete_json("review", "payload")
        .await
        .expect_err("tool events fail closed");
    assert!(matches!(
        err,
        LlmError::Backend {
            kind: BackendErrorKind::Contract,
            ..
        }
    ));
    drop(dir);
}

#[tokio::test]
async fn unknown_nonzero_exit_is_not_classified_from_stderr_prose() {
    let (dir, client) = fake_client(
        concat!(
            "#!/bin/sh\n",
            "sed -n '1,$p' >/dev/null\n",
            "printf '%s\\n' '{\"type\":\"error\",\"message\":\"machine-readable terminal detail\"}'\n",
            "printf 'unauthorized timeout quota\\033[31m' >&2\n",
            "exit 19\n",
        ),
        5,
    );
    let err = client
        .complete_json("review", "payload")
        .await
        .expect_err("nonzero exit");
    match err {
        LlmError::Backend {
            kind: BackendErrorKind::UnknownExit,
            message,
        } => {
            assert!(message.contains("status 19"), "{message}");
            assert!(
                message.contains("machine-readable terminal detail"),
                "{message}"
            );
            assert!(!message.chars().any(char::is_control), "{message:?}");
        }
        other => panic!("unexpected classification: {other:?}"),
    }
    drop(dir);
}

#[tokio::test]
async fn a_large_stderr_is_drained_but_only_a_bounded_excerpt_is_reported() {
    let (dir, client) = fake_client(
        "#!/bin/sh\ndd if=/dev/zero bs=1024 count=40 2>/dev/null | tr '\\000' x >&2\nprintf tail-marker >&2\nexit 19\n",
        5,
    );
    let err = client
        .complete_json("review", "payload")
        .await
        .expect_err("nonzero exit");
    match err {
        LlmError::Backend {
            kind: BackendErrorKind::UnknownExit,
            message,
        } => {
            assert!(
                message.len() < 600,
                "stderr was not bounded: {}",
                message.len()
            );
            assert!(!message.contains("tail-marker"), "got {message}");
        }
        other => panic!("unexpected classification: {other:?}"),
    }
    drop(dir);
}

#[tokio::test]
async fn malformed_final_json_stays_an_unparseable_model_response() {
    let (dir, client) = fake_client(
        concat!(
            "#!/bin/sh\n",
            "sed -n '1,$p' >/dev/null\n",
            "printf '%s\\n' \\\n",
            "  '{\"type\":\"item.completed\",\"item\":{\"type\":\"agent_message\",\"text\":\"not json\"}}' \\\n",
            "  '{\"type\":\"turn.completed\"}'\n",
        ),
        5,
    );
    let err = client
        .complete_json("review", "payload")
        .await
        .expect_err("final message is malformed");
    assert!(matches!(err, LlmError::Unparseable(_)), "got {err:?}");
    drop(dir);
}

#[tokio::test]
async fn stdout_overflow_is_a_bounded_transport_failure() {
    let (dir, client) = fake_client(
        "#!/bin/sh\ndd if=/dev/zero bs=1048576 count=17 2>/dev/null\n",
        5,
    );
    let err = client
        .complete_json("review", "payload")
        .await
        .expect_err("stdout exceeds its bound");
    assert!(
        matches!(err, LlmError::Transport { status: None, ref message } if message.contains("exceeded")),
        "got {err:?}"
    );
    drop(dir);
}

#[tokio::test]
async fn process_stdout_accepts_exactly_sixteen_mebibytes() {
    const EXPECTED_LIMIT: usize = 16 * 1024 * 1024;
    let dir = tempfile::tempdir().expect("tempdir");
    let executable = dir.path().join("fake-codex");
    let output_path = dir.path().join("stdout");
    std::fs::write(&output_path, vec![b'x'; EXPECTED_LIMIT]).expect("bounded stdout fixture");
    crate::test_support::write_executable(
        &executable,
        "#!/bin/sh\ncapture=$(dirname \"$0\")\nexec /bin/cat \"$capture/stdout\"\n",
    );

    let output = process::run(
        &executable,
        &[],
        &ChildEnvironment::default(),
        dir.path(),
        "",
        Duration::from_secs(5),
    )
    .await
    .expect("the exact stdout ceiling is accepted");

    assert_eq!(output.stdout.len(), EXPECTED_LIMIT);
}

#[tokio::test]
async fn process_stderr_retains_exactly_thirty_two_kibibytes_while_draining() {
    const EXPECTED_LIMIT: usize = 32 * 1024;
    let dir = tempfile::tempdir().expect("tempdir");
    let executable = dir.path().join("fake-codex");
    let stderr_path = dir.path().join("stderr");
    std::fs::write(&stderr_path, vec![b'x'; EXPECTED_LIMIT + 1024]).expect("noisy stderr fixture");
    crate::test_support::write_executable(
        &executable,
        "#!/bin/sh\ncapture=$(dirname \"$0\")\n/bin/cat \"$capture/stderr\" >&2\n",
    );

    let output = process::run(
        &executable,
        &[],
        &ChildEnvironment::default(),
        dir.path(),
        "",
        Duration::from_secs(5),
    )
    .await
    .expect("noisy stderr is drained");

    assert_eq!(output.stderr.len(), EXPECTED_LIMIT);
}

#[tokio::test]
async fn process_missing_binary_is_a_configuration_failure() {
    let dir = tempfile::tempdir().expect("tempdir");
    let result = process::run(
        &dir.path().join("missing-codex"),
        &[],
        &ChildEnvironment::default(),
        dir.path(),
        "",
        Duration::from_secs(5),
    )
    .await;
    let err = match result {
        Err(err) => err,
        Ok(_) => panic!("missing binary unexpectedly ran"),
    };

    assert!(
        matches!(err, LlmError::NotConfigured(ref message) if message.contains("not found")),
        "got {err:?}"
    );
}

#[tokio::test]
async fn a_child_that_closes_stdin_early_is_a_transport_failure() {
    let (dir, client) = fake_client("#!/bin/sh\nexit 0\n", 5);
    let payload = "x".repeat(1024 * 1024);
    let err = client
        .complete_json("review", &payload)
        .await
        .expect_err("the payload was not delivered");
    assert!(
        matches!(err, LlmError::Transport { status: None, ref message } if message.contains("send the review payload")),
        "got {err:?}"
    );
    drop(dir);
}

#[cfg(unix)]
#[tokio::test]
async fn a_signal_terminated_child_is_a_transport_failure() {
    let (dir, client) = fake_client("#!/bin/sh\nkill -TERM $$\n", 5);
    let err = client
        .complete_json("review", "payload")
        .await
        .expect_err("signal termination has no exit code");
    assert!(matches!(err, LlmError::Transport { status: None, .. }));
    drop(dir);
}

#[tokio::test]
async fn timeout_kills_the_child_before_it_can_continue() {
    let (dir, client) = fake_client(
        "#!/bin/sh\ncapture=$(dirname \"$0\")\nsleep 5\nprintf late > \"$capture/late\"\n",
        1,
    );
    let err = client
        .complete_json("review", "payload")
        .await
        .expect_err("child times out");
    assert!(matches!(err, LlmError::Transport { status: None, .. }));
    assert!(
        !dir.path().join("late").exists(),
        "timed-out child continued"
    );
}

fn fake_client(script: &str, timeout_secs: u64) -> (tempfile::TempDir, CodexClient) {
    let dir = tempfile::tempdir().expect("tempdir");
    let executable = dir.path().join("fake-codex");
    crate::test_support::write_executable(&executable, script);
    let cfg = LlmConfig {
        backend: BackendKind::Codex,
        model: Some("gpt-5.6-sol".to_owned()),
        reasoning_effort: Some(ReasoningEffort::High),
        timeout_secs,
        max_concurrent: 1,
        ..LlmConfig::default()
    };
    let client = CodexClient::for_test(
        &cfg,
        executable,
        [
            (OsString::from("PATH"), OsString::from("/usr/bin:/bin")),
            (OsString::from("HOME"), OsString::from("/safe/home")),
        ],
        "0.148.0",
    )
    .expect("test client");
    (dir, client)
}
