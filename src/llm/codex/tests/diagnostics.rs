//! Only the CLI version and ChatGPT-auth readiness survive diagnostic parsing.

use crate::llm::codex::command::ChildEnvironment;
use crate::llm::codex::diagnostics::{
    DIAGNOSTIC_TIMEOUT, DiagnosticError, parse_login_status, parse_version, poll_before_deadline,
    probe,
};

#[test]
fn version_parser_extracts_clean_cli_version_and_rejects_malformed_or_hostile_text() {
    assert_eq!(parse_version(b"codex-cli 0.171.0\n").unwrap(), "0.171.0");
    assert_eq!(parse_version(b"codex-cli 0.171.0\r\n").unwrap(), "0.171.0");
    for bad in [
        b"" as &[u8],
        b"codex-cli \n",
        b"codex-cli 0.171.0 extra\n",
        b"other-cli 0.171.0\n",
        b"codex-cli 0.171.0\x1b]52;c;Zm9v\x07\n",
        b"codex-cli 0.171.0\nsecond-line\n",
        b"\xff\xfe",
    ] {
        assert_eq!(
            parse_version(bad),
            Err(DiagnosticError::UnsupportedFormat),
            "rejected input: {bad:?}"
        );
    }
}

#[test]
fn login_status_parser_requires_success_and_chatgpt_login_without_echoing_details() {
    assert_eq!(
        parse_login_status(true, b"Logged in using ChatGPT\n"),
        Ok(())
    );
    assert_eq!(
        parse_login_status(true, b"warning: notice\nLogged in using ChatGPT\n"),
        Ok(())
    );

    let api_err = parse_login_status(true, b"API key configured - sk-secret\n")
        .expect_err("API auth cannot fund subscription usage");
    assert_eq!(api_err, DiagnosticError::NotChatGpt);
    assert!(!api_err.to_string().contains("sk-secret"));

    assert_eq!(
        parse_login_status(true, b""),
        Err(DiagnosticError::NotChatGpt)
    );
    assert_eq!(
        parse_login_status(false, b"Not logged in\n"),
        Err(DiagnosticError::NotChatGpt)
    );
    assert_eq!(
        parse_login_status(false, b"Logged in using ChatGPT\n"),
        Err(DiagnosticError::NotChatGpt)
    );
    assert_eq!(
        parse_login_status(false, b""),
        Err(DiagnosticError::Process)
    );
    assert_eq!(
        parse_login_status(true, b"\xff\xfe"),
        Err(DiagnosticError::UnsupportedFormat)
    );
}

#[test]
fn a_missing_binary_has_actionable_redacted_guidance() {
    let err = probe(
        std::path::Path::new("/definitely-not-a-real-drep-codex-binary"),
        &ChildEnvironment::default(),
        DIAGNOSTIC_TIMEOUT,
    )
    .expect_err("binary is absent");

    assert!(matches!(err, DiagnosticError::MissingBinary));
    assert!(err.to_string().contains("`codex login`"));
}

#[test]
#[cfg(unix)]
fn probe_reads_version_from_stdout_and_chatgpt_login_from_stderr() {
    let dir = tempfile::tempdir().expect("tempdir");
    let executable = dir.path().join("fake-codex");
    let args_log = dir.path().join("args.txt");
    crate::test_support::write_executable(
        &executable,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nif [ \"$1\" = \"--version\" ]; then\n  printf 'warning: noisy stderr\\n' >&2\n  printf 'codex-cli 0.171.0\\n'\n  exit 0\nfi\nprintf 'Logged in using ChatGPT\\n' >&2\nexit 0\n",
            args_log.display()
        ),
    );

    let status = probe(
        &executable,
        &ChildEnvironment::default(),
        DIAGNOSTIC_TIMEOUT,
    )
    .expect("valid version and ChatGPT login");
    assert_eq!(status.cli_version(), "0.171.0");
    assert_eq!(
        format!("{status:?}"),
        "CodexStatus { cli_version: \"0.171.0\" }"
    );
    let recorded = std::fs::read_to_string(&args_log).expect("recorded args");
    let lines: Vec<&str> = recorded.lines().collect();
    assert_eq!(
        lines,
        vec![
            "--version",
            "-c forced_login_method=\"chatgpt\" login status"
        ]
    );
}

#[test]
#[cfg(unix)]
fn a_failing_version_command_is_a_process_failure() {
    let dir = tempfile::tempdir().expect("tempdir");
    let executable = dir.path().join("fake-codex");
    crate::test_support::write_executable(&executable, "#!/bin/sh\nexit 1\n");

    assert_eq!(
        probe(
            &executable,
            &ChildEnvironment::default(),
            DIAGNOSTIC_TIMEOUT
        ),
        Err(DiagnosticError::Process)
    );
}

#[test]
#[cfg(unix)]
fn an_empty_nonzero_login_status_is_a_process_failure() {
    let dir = tempfile::tempdir().expect("tempdir");
    let executable = dir.path().join("fake-codex");
    crate::test_support::write_executable(
        &executable,
        "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then\n  printf 'codex-cli 0.171.0\\n'\n  exit 0\nfi\nexit 1\n",
    );

    assert_eq!(
        probe(
            &executable,
            &ChildEnvironment::default(),
            DIAGNOSTIC_TIMEOUT
        ),
        Err(DiagnosticError::Process)
    );
}

#[test]
#[cfg(unix)]
fn diagnostic_output_accepts_exactly_one_mebibyte_and_rejects_one_byte_over() {
    const EXPECTED_LIMIT: usize = 1024 * 1024;
    let dir = tempfile::tempdir().expect("tempdir");

    let exact = diagnostic_executable(dir.path(), "exact", login_payload_with_len(EXPECTED_LIMIT));
    let status = probe(&exact, &ChildEnvironment::default(), DIAGNOSTIC_TIMEOUT)
        .expect("the exact diagnostic ceiling is accepted");
    assert_eq!(status.cli_version(), "0.171.0");

    let over = diagnostic_executable(
        dir.path(),
        "over",
        login_payload_with_len(EXPECTED_LIMIT + 1),
    );
    assert_eq!(
        probe(&over, &ChildEnvironment::default(), DIAGNOSTIC_TIMEOUT),
        Err(DiagnosticError::OutputTooLarge)
    );
}

#[cfg(unix)]
#[test]
fn a_grandchild_inheriting_stdout_cannot_hold_the_diagnostic_open() {
    let dir = tempfile::tempdir().expect("tempdir");
    let executable = dir.path().join("fake-codex");
    crate::test_support::write_executable(
        &executable,
        "#!/bin/sh\ncapture=$(dirname \"$0\")\nif [ \"$1\" = \"--version\" ]; then\n  sleep 30 &\n  printf '%s' \"$!\" > \"$capture/grandchild.pid\"\n  printf 'codex-cli 0.171.0\\n'\n  exit 0\nfi\nprintf 'Logged in using ChatGPT\\n' >&2\nexit 0\n",
    );

    let status = probe(
        &executable,
        &ChildEnvironment::default(),
        DIAGNOSTIC_TIMEOUT,
    )
    .expect("valid diagnostic");
    let pid = std::fs::read_to_string(dir.path().join("grandchild.pid")).expect("grandchild pid");
    let running = crate::test_support::probe_and_stop_process(&pid);

    assert_eq!(status.cli_version(), "0.171.0");
    assert!(running, "probe waited for the unrelated grandchild to exit");
}

#[test]
fn diagnostic_deadlines_stop_polling_and_terminate_children() {
    let deadline = std::time::Duration::from_secs(30);
    assert!(poll_before_deadline(
        std::time::Duration::from_secs(29),
        deadline
    ));
    assert!(!poll_before_deadline(deadline, deadline));

    #[cfg(unix)]
    {
        let mut child = std::process::Command::new("/bin/sleep")
            .arg("30")
            .spawn()
            .expect("sleep child");
        crate::llm::codex::diagnostics::stop(&mut child);
        let stopped = child.try_wait();
        // Clean up before asserting, including when stop is broken.
        let _ = child.kill();
        let _ = child.wait();
        assert!(
            matches!(stopped, Ok(Some(status)) if !status.success()),
            "stop must terminate the child"
        );

        let dir = tempfile::tempdir().expect("tempdir");
        let executable = dir.path().join("fake-codex");
        // The healthy runner kills this child immediately; the finite sleep
        // also bounds the test when the polling guard is broken.
        crate::test_support::write_executable(&executable, "#!/bin/sh\nexec /bin/sleep 30\n");
        assert!(matches!(
            probe(
                &executable,
                &ChildEnvironment::default(),
                std::time::Duration::ZERO
            ),
            Err(DiagnosticError::Timeout)
        ));
    }
}

fn login_payload_with_len(length: usize) -> Vec<u8> {
    let suffix = b"\nLogged in using ChatGPT\n";
    let padding = length
        .checked_sub(suffix.len())
        .expect("requested size fits");
    let mut bytes = vec![b'x'; padding];
    bytes.extend_from_slice(suffix);
    assert_eq!(bytes.len(), length);
    bytes
}

#[cfg(unix)]
fn diagnostic_executable(
    dir: &std::path::Path,
    label: &str,
    login_bytes: Vec<u8>,
) -> std::path::PathBuf {
    let executable = dir.join(format!("fake-codex-{label}"));
    let payload = dir.join(format!("login-{label}.txt"));
    std::fs::write(&payload, login_bytes).expect("login fixture");
    crate::test_support::write_executable(
        &executable,
        format!(
            "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then\n  printf 'codex-cli 0.171.0\\n'\n  exit 0\nfi\nexec /bin/cat '{}'\n",
            payload.display()
        ),
    );
    executable
}
