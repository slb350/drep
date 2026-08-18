//! Output rendering: criteria 19-23.
//!
//! These tests need to assert on what `drep check` actually prints, and
//! `render::render` writes to stdout directly - so the test cannot see
//! the output without redirecting it. The only way to capture stdout
//! without touching production code is to spawn the drep binary as a
//! subprocess. Every test in this file does that, captures stdout, and
//! then asserts on the captured bytes.
//!
//! Five criteria, five tests:
//!
//! - 19: exact text line for a known finding.
//! - 20: clean run's text is exactly `No issues found.\n`.
//! - 21: JSON `unanalyzed` is **present** (not falsy) on a clean run.
//! - 22: JSON `findings[].source` distinguishes tool from llm.
//! - 23: JSON `exit` matches the `Exit`'s code on a run with failures.

use std::path::Path;
use std::process::Command;

use serde_json::Value;
use wiremock::MockServer;

use crate::test_support::make_executable;
use crate::test_support::mount_sse;
use crate::test_support::sse;

// ---------- scaffolding ----------

/// Spawn the drep binary with `cwd = dir` and `args`, returning the
/// captured `Output` so callers can inspect stdout/stderr/status.
///
/// `HOME` is pointed at `dir` so the LLM cache lives under the test's
/// `TempDir` rather than the real `~/Library/Caches`. Without this,
/// `check::run`'s use of `Cache::default_root()` would carry cached
/// responses across tests - which means test 14's "something smells"
/// response leaks into test 19, which expects a different finding for
/// the same payload (same file content, same model, same temperature).
fn run_drep(dir: &Path, args: &[&str]) -> std::process::Output {
    let bin = assert_cmd::cargo::cargo_bin("drep");
    Command::new(bin)
        .args(args)
        .current_dir(dir)
        .env("HOME", dir)
        .env("XDG_CACHE_HOME", dir)
        .output()
        .expect("drep spawns")
}

/// Mount one SSE response on `server` with `body` as the JSON payload.
async fn mount_llm(server: &MockServer, body: &str) {
    mount_sse(
        server,
        wiremock::ResponseTemplate::new(200).set_body_raw(sse(&[body]), "text/event-stream"),
    )
    .await;
}

// ---------- 19 ----------

/// Criterion 19: the text output for a known finding equals the exact
/// expected string.
///
/// Source prefix (`llm/`), severity, position, message, and the indented
/// suggestion line - all in one string. Asserting the whole string
/// (rather than `contains`) is what catches a regression that drops the
/// suggestion, drops the source prefix, or flips the field order.
#[test]
fn text_output_for_a_known_finding_is_exactly_the_expected_string() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    let (dir, _server) = runtime.block_on(async {
        let dir = tempfile::tempdir().expect("tempdir");
        let server = MockServer::start().await;
        mount_llm(
            &server,
            r#"{"issues":[{"line":1,"severity":"critical","category":"bug","message":"test message","suggestion":"fix it"}]}"#,
        )
        .await;
        crate::test_support::write_drep_toml(dir.path(), &format!("{}/v1", server.uri()));
        std::fs::write(dir.path().join("lib.py"), "x = 1\n").expect("lib.py");
        (dir, server)
    });

    let output = run_drep(dir.path(), &["check", "lib.py"]);
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();

    assert_eq!(
        output.status.code(),
        Some(0),
        "expected Clean exit, stderr was {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    let expected = "lib.py:1: error [llm/bug] test message\n    suggestion: fix it\n";
    assert_eq!(
        stdout, expected,
        "rendered text must equal the exact expected string"
    );
}

// ---------- 20 ----------

/// Criterion 20: a clean run's text output is exactly `No issues found.\n`.
///
/// Not `"No issues found."` (no newline) and not `"No issues found.\n\n"`.
/// The byte-exactness is what a downstream consumer can rely on.
#[test]
fn clean_run_text_output_is_exactly_no_issues_found() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    // Keep both the dir AND the mock server alive for the duration of
    // the subprocess. Dropping the server before `run_drep` frees the
    // port and the subprocess would fail to connect, exit 2 instead of
    // the expected exit 0.
    let (dir, _server) = runtime.block_on(async {
        let dir = tempfile::tempdir().expect("tempdir");
        let server = MockServer::start().await;
        mount_llm(&server, r#"{"issues":[]}"#).await;
        crate::test_support::write_drep_toml(dir.path(), &format!("{}/v1", server.uri()));
        std::fs::write(dir.path().join("lib.py"), "x = 1\n").expect("lib.py");
        (dir, server)
    });

    let output = run_drep(dir.path(), &["check", "lib.py"]);
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();

    assert_eq!(
        output.status.code(),
        Some(0),
        "expected Clean exit, stderr was {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        stdout, "No issues found.\n",
        "clean run text must be exactly 'No issues found.\\n'"
    );
}

// ---------- 21 ----------

/// Criterion 21: JSON parses and the `unanalyzed` key is **present**
/// (not falsy) on a clean run.
///
/// The test asserts the key exists (`obj.get(...).is_some()`), not that
/// the value is `[]`. A consumer must be able to tell "no failures" from
/// "this build of drep does not report them" - which the absent key
/// encodes as `null` in serde_json's eyes but as missing in the caller's
/// eyes.
#[test]
fn json_clean_run_has_unanalyzed_key_present_as_empty_array() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    let (dir, _server) = runtime.block_on(async {
        let dir = tempfile::tempdir().expect("tempdir");
        let server = MockServer::start().await;
        mount_llm(&server, r#"{"issues":[]}"#).await;
        crate::test_support::write_drep_toml(dir.path(), &format!("{}/v1", server.uri()));
        std::fs::write(dir.path().join("lib.py"), "x = 1\n").expect("lib.py");
        (dir, server)
    });

    let output = run_drep(dir.path(), &["check", "--format", "json", "lib.py"]);
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();

    assert_eq!(
        output.status.code(),
        Some(0),
        "expected Clean exit, stderr was {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    let parsed: Value = serde_json::from_str(&stdout).expect("json parses");
    let obj = parsed.as_object().expect("json object");
    assert!(
        obj.get("unanalyzed").is_some(),
        "unanalyzed must be present in clean-run JSON; got {stdout:?}"
    );
    assert_eq!(
        obj["unanalyzed"],
        Value::Array(Vec::new()),
        "unanalyzed must be [] on a clean run, got {:?}",
        obj["unanalyzed"]
    );
}

// ---------- 22 ----------

/// Criterion 22: JSON `findings[].source` is `"tool"` for a tool finding
/// and `"llm"` for an LLM finding.
///
/// Both in one test, so a renderer that hardcoded one of them cannot
/// pass. The fake `ruff` produces a tool finding at `lib.py:1`; the mock
/// LLM produces an LLM finding at `lib.py:2`.
#[test]
fn json_findings_distinguish_tool_from_llm_via_source_field() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    let (dir, _server) = runtime.block_on(async {
        let dir = tempfile::tempdir().expect("tempdir");
        let server = MockServer::start().await;
        mount_llm(
            &server,
            r#"{"issues":[{"line":2,"severity":"high","category":"perf","message":"llm-msg","suggestion":""}]}"#,
        )
        .await;
        crate::test_support::write_drep_toml(dir.path(), &format!("{}/v1", server.uri()));

        // Configure ruff and plant a fake binary that emits one finding.
        std::fs::write(dir.path().join("pyproject.toml"), "").expect("pyproject");
        let bin = dir.path().join("venv/bin/ruff");
        std::fs::create_dir_all(bin.parent().unwrap()).expect("bin dir");
        std::fs::write(
            &bin,
            "#!/bin/sh\nprintf '%s' '[{\"code\":\"F401\",\"filename\":\"lib.py\",\"location\":{\"row\":1,\"column\":1},\"message\":\"tool-msg\"}]'\n",
        )
        .expect("ruff");
        make_executable(&bin);

        std::fs::write(dir.path().join("lib.py"), "x = 1\ny = 2\n").expect("lib.py");
        (dir, server)
    });

    let output = run_drep(dir.path(), &["check", "--format", "json", "lib.py"]);
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();

    assert_eq!(
        output.status.code(),
        Some(1),
        "expected FoundIssues exit (tool finding blocks), stderr was {:?}",
        String::from_utf8_lossy(&output.stderr)
    );

    let parsed: Value = serde_json::from_str(&stdout).expect("json parses");
    let findings = parsed["findings"]
        .as_array()
        .expect("findings array")
        .clone();

    let mut saw_tool = false;
    let mut saw_llm = false;
    for f in &findings {
        match f["source"].as_str() {
            Some("tool") => saw_tool = true,
            Some("llm") => saw_llm = true,
            other => panic!("unexpected source {other:?} in finding {f:?}"),
        }
    }
    assert!(saw_tool, "must have a tool finding, got {findings:?}");
    assert!(saw_llm, "must have an llm finding, got {findings:?}");
}

// ---------- 23 ----------

/// Criterion 23: JSON `exit` equals the returned `Exit`'s code on a run
/// with failures.
///
/// Triggering a failure is cheap: a non-UTF-8 file in the work set. The
/// file is rejected at the input layer and surfaces as `Unanalyzed`
/// (exit 2). The JSON must report the same `2`.
#[test]
fn json_exit_matches_returned_exit_code_on_a_run_with_failures() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    let (dir, _server) = runtime.block_on(async {
        let dir = tempfile::tempdir().expect("tempdir");
        let server = MockServer::start().await;
        mount_llm(&server, r#"{"issues":[]}"#).await;
        crate::test_support::write_drep_toml(dir.path(), &format!("{}/v1", server.uri()));
        // Bad bytes for the input layer to refuse. The file would be
        // sent to the LLM if it were UTF-8, but `std::fs::read_to_string`
        // rejects it and the orchestrator records `Unreadable`.
        std::fs::write(dir.path().join("bad.py"), [0xFFu8, 0xFE, 0xFD]).expect("bad.py");
        (dir, server)
    });

    let output = run_drep(dir.path(), &["check", "--format", "json", "bad.py"]);
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();

    assert_eq!(
        output.status.code(),
        Some(2),
        "expected Unanalyzed exit, stderr was {:?}",
        String::from_utf8_lossy(&output.stderr)
    );

    let parsed: Value = serde_json::from_str(&stdout).expect("json parses");
    assert_eq!(
        parsed["exit"].as_u64(),
        Some(2),
        "JSON exit must match the returned Exit::Unanalyzed code"
    );
}

/// The JSON `exit` must equal the process exit code in the case where a
/// second, divergent computation would disagree.
///
/// A failure run cannot show this: every way of computing the exit agrees
/// that a failure is 2, which is why criterion 23 passed against a `render`
/// that recomputed the verdict itself and ignored `--fail-on`. The case that
/// separates them is an LLM finding with **no** `--fail-on`: the gate says
/// clean (0) because LLM findings only inform, while "any finding means 1"
/// says 1. The gate is the single source of truth and `render` is handed its
/// verdict.
#[test]
fn json_exit_matches_the_gate_when_an_llm_finding_does_not_block() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    let (dir, _server) = runtime.block_on(async {
        let dir = tempfile::tempdir().expect("tempdir");
        let server = MockServer::start().await;
        // One real LLM finding, at the file's only line.
        mount_llm(
            &server,
            r#"{"issues":[{"line":1,"severity":"high","category":"bug","message":"m"}],"summary":""}"#,
        )
        .await;
        crate::test_support::write_drep_toml(dir.path(), &format!("{}/v1", server.uri()));
        std::fs::write(dir.path().join("only.py"), "x = 1\n").expect("only.py");
        (dir, server)
    });

    // No --fail-on, so the LLM finding informs and must not block.
    let output = run_drep(dir.path(), &["check", "--format", "json", "only.py"]);
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let parsed: Value = serde_json::from_str(&stdout).expect("json parses");

    assert_eq!(
        output.status.code(),
        Some(0),
        "an LLM finding must not block without --fail-on; stderr {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        parsed["exit"].as_u64(),
        Some(0),
        "the JSON exit must be the gate's verdict, not a recomputation that \
         treats any finding as blocking; got {stdout}"
    );
    assert!(
        !parsed["findings"]
            .as_array()
            .expect("findings array")
            .is_empty(),
        "the finding must still be reported, just not blocking: {stdout}"
    );
}

/// Each suggestion is written directly beneath the finding it belongs to.
///
/// The renderer used to print every finding line and then every suggestion
/// line, so with two findings the first suggestion appeared below the second
/// finding and read as if it belonged to it. Only a single-finding fixture
/// could miss this, which is what criterion 19 uses.
#[test]
fn each_suggestion_follows_its_own_finding() {
    use crate::analysis::findings::{Finding, Severity};
    use crate::cli::OutputFormat;
    use crate::cli::check::{CheckOutcome, render};
    use std::collections::BTreeMap;

    let finding = |line: u32, message: &str, suggestion: &str| Finding {
        kind: "bug".to_owned(),
        severity: Severity::Error,
        file_path: "src/lib.rs".to_owned(),
        line,
        column: None,
        message: message.to_owned(),
        suggestion: Some(suggestion.to_owned()),
    };

    let outcome = CheckOutcome {
        tool_findings: vec![
            finding(1, "first", "fix one"),
            finding(2, "second", "fix two"),
        ],
        llm_findings: Vec::new(),
        failures: BTreeMap::new(),
        exit: crate::Exit::FoundIssues,
    };

    let mut buf: Vec<u8> = Vec::new();
    render::render_to(&mut buf, &outcome, OutputFormat::Text).expect("render");
    let text = String::from_utf8(buf).expect("utf8");

    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(
        lines,
        vec![
            "src/lib.rs:1: error [tool/bug] first",
            "    suggestion: fix one",
            "src/lib.rs:2: error [tool/bug] second",
            "    suggestion: fix two",
        ],
        "each suggestion must directly follow its own finding, got:\n{text}"
    );
}
