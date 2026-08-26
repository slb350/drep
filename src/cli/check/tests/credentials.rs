//! Credential resolution inside the orchestrator.
//!
//! The point of the whole feature is that a credential drep could not resolve is
//! exit 2, not a provider drep quietly asks instead. Both halves are pinned
//! here: the run fails, and the endpoint is never contacted - because "fell over
//! to the next provider" and "sent the code somewhere before giving up" are the
//! two ways a broken credential path stops being visible.

use std::path::Path;

use wiremock::MockServer;

use super::support::check_args as args;
use crate::cli::MachineFiles;
use crate::cli::check;
use crate::llm::cache::Cache;
use crate::test_support::{mount_sse, sse, write_executable};

/// Write a `drep.toml` whose only provider mints its key by running `argv`.
fn write_config_running(dir: &Path, endpoint: &str, argv: &[&Path]) {
    let argv: Vec<String> = argv
        .iter()
        .map(|p| format!("{:?}", p.to_string_lossy()))
        .collect();
    let body = format!(
        "[[llm]]\nendpoint = \"{endpoint}\"\nmodel = \"m\"\n\
         api_key_command = [{}]\nmax_retries = 1\n",
        argv.join(", ")
    );
    std::fs::write(dir.join("drep.toml"), body).expect("drep.toml");
}

#[tokio::test]
async fn a_failing_api_key_command_exits_two_without_contacting_the_endpoint() {
    let dir = tempfile::tempdir().expect("tempdir");
    let server = MockServer::start().await;
    mount_sse(
        &server,
        wiremock::ResponseTemplate::new(200)
            .set_body_raw(sse(&[r#"{"issues": []}"#]), "text/event-stream"),
    )
    .await;
    let stub = dir.path().join("print-token");
    write_executable(&stub, "#!/bin/sh\nexit 4\n");
    write_config_running(dir.path(), &format!("{}/v1", server.uri()), &[&stub]);
    let source = dir.path().join("lib.py");
    std::fs::write(&source, "x = 1\n").expect("lib.py");

    let result = check::run_against(
        &args(vec![source], None),
        dir.path(),
        Cache::new(dir.path().join("test-cache"), 30, 8 * 1024 * 1024),
        &MachineFiles {
            auth: &dir.path().join("auth.toml"),
            policy: &dir.path().join("absent-site.toml"),
        },
    )
    .await;

    let err = result.expect_err("a credential drep could not resolve is never a clean run");
    let message = format!("{err:#}");
    assert!(message.contains("api_key_command"), "got {message}");
    assert!(
        server
            .received_requests()
            .await
            .is_some_and(|requests| requests.is_empty()),
        "the gate must fail before a byte of source is sent anywhere"
    );
}

#[tokio::test]
async fn a_working_api_key_command_lets_the_run_complete() {
    // The discriminating half: without it, "always fail when a command is
    // configured" passes the test above.
    let dir = tempfile::tempdir().expect("tempdir");
    let server = MockServer::start().await;
    mount_sse(
        &server,
        wiremock::ResponseTemplate::new(200)
            .set_body_raw(sse(&[r#"{"issues": []}"#]), "text/event-stream"),
    )
    .await;
    let stub = dir.path().join("print-token");
    write_executable(&stub, "#!/bin/sh\necho minted-token\n");
    write_config_running(dir.path(), &format!("{}/v1", server.uri()), &[&stub]);
    let source = dir.path().join("lib.py");
    std::fs::write(&source, "x = 1\n").expect("lib.py");

    let exit = check::run_against(
        &args(vec![source], None),
        dir.path(),
        Cache::new(dir.path().join("test-cache"), 30, 8 * 1024 * 1024),
        &MachineFiles {
            auth: &dir.path().join("auth.toml"),
            policy: &dir.path().join("absent-site.toml"),
        },
    )
    .await
    .expect("a minted credential is a working one");

    assert_eq!(exit, check::Exit::Clean);
}
