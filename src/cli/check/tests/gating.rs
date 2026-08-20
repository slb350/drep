//! Gating and exit codes: criteria 12-18.
//!
//! Seven tests, one precedence table:
//!
//! - Exit 0 (`Clean`): no failures, no blocking findings.
//! - Exit 1 (`FoundIssues`): tool findings always block; LLM findings block
//!   when `--fail-on` admits their severity and is set.
//! - Exit 2 (`Unanalyzed`): any failure outranks any finding.
//!
//! Tests 12-17 are in-process: they call `check::run` directly and assert
//! only on the returned `Exit`. Tests 14 and 18 also assert on the
//! rendered output (a finding must be visible in text output, and an
//! unreachable endpoint must not print "No issues found."), so they
//! spawn the drep binary as a subprocess to capture stdout.

use std::path::Path;
use std::process::Command;

use wiremock::MockServer;

use crate::analysis::findings::Severity;
use crate::cli::OutputFormat;
use crate::cli::check::{self, CheckArgs};
use crate::llm::cache::Cache;
use crate::test_support::mount_sse;
use crate::test_support::sse;
use crate::test_support::write_executable;

// ---------- in-process scaffolding ----------

/// Build a fresh fixture: an empty `TempDir` and a `MockServer` mounted
/// with one SSE response. The fixture does **not** write `drep.toml` -
/// the per-test helper does, so each test names its own endpoint.
async fn setup_mock(body: &str) -> (tempfile::TempDir, MockServer) {
    let dir = tempfile::tempdir().expect("tempdir");
    let server = MockServer::start().await;
    mount_sse(
        &server,
        wiremock::ResponseTemplate::new(200).set_body_raw(sse(&[body]), "text/event-stream"),
    )
    .await;
    (dir, server)
}

/// Build a `CheckArgs` for paths mode with an optional gating threshold.
fn args(paths: Vec<std::path::PathBuf>, fail_on: Option<Severity>) -> CheckArgs {
    CheckArgs {
        paths,
        staged: false,
        diff: None,
        tip: None,
        format: OutputFormat::Text,
        fail_on,
    }
}

/// Run `check` in-process against a cache scoped to this test.
///
/// Goes through `run_with`, not `run`. `run` builds `Cache::default_root()` -
/// the developer's real `~/Library/Caches` - so every in-process test wrote
/// there and could be satisfied by an entry another test left behind. The
/// seam existed for exactly this and had no callers.
async fn run_paths_with(args: &CheckArgs, dir: &Path) -> check::Exit {
    let cache = Cache::new(dir.join("test-cache"), 30, 8 * 1024 * 1024);
    check::run_with(args, dir, cache)
        .await
        .expect("check::run succeeds")
}

/// The common case: one path, default format, no gating threshold.
async fn run_paths(path: std::path::PathBuf, dir: &Path) -> check::Exit {
    run_paths_with(&args(vec![path], None), dir).await
}

// ---------- subprocess scaffolding ----------

/// Spawn the drep binary with `cwd = dir`, capture stdout/stderr/exit.
///
/// `HOME` is pointed at `dir` so the LLM cache lives under this test's
/// `TempDir` rather than the real `~/Library/Caches`. Without this, an
/// earlier in-process test (which uses `Cache::default_root()`) would
/// have cached a clean response for the same payload, and this test
/// would see that hit instead of the unreachable-endpoint error it is
/// trying to pin.
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

// ---------- 12 ----------

/// Criterion 12: a clean run returns `Exit::Clean`.
///
/// "Clean" requires *both* layers to come back empty and no failures to
/// land on any file. The mock returns no issues; the project has no
/// `pyproject.toml` so ruff is `Skipped`; there are no read failures.
/// Either layer reporting a finding or a failure breaks the assertion.
#[tokio::test]
async fn clean_run_returns_clean() {
    let (dir, server) = setup_mock(r#"{"issues": []}"#).await;
    crate::test_support::write_drep_toml(dir.path(), &format!("{}/v1", server.uri()));
    std::fs::write(dir.path().join("lib.py"), "x = 1\n").expect("lib.py");

    let exit = run_paths(dir.path().join("lib.py"), dir.path()).await;
    assert_eq!(exit, check::Exit::Clean, "clean run must return Clean");
    assert_eq!(exit.code(), 0, "Clean must map to exit 0");
}

#[tokio::test]
async fn a_completed_run_enforces_the_cache_size_ceiling() {
    let (dir, server) = setup_mock(r#"{"issues": []}"#).await;
    crate::test_support::write_drep_toml(dir.path(), &format!("{}/v1", server.uri()));
    let source = dir.path().join("lib.py");
    std::fs::write(&source, "x = 1\n").expect("lib.py");
    let cache_root = dir.path().join("tiny-cache");
    let cache = Cache::new(cache_root.clone(), 30, 1);

    let exit = check::run_with(&args(vec![source], None), dir.path(), cache)
        .await
        .expect("check succeeds");

    assert_eq!(exit, check::Exit::Clean);
    let bytes = crate::test_support::two_level_tree_size(&cache_root);
    assert!(bytes <= 1, "cache retained {bytes} bytes past its ceiling");
}

// ---------- 13 ----------

/// Criterion 13: a tool finding blocks with no `--fail-on`.
///
/// The mock is mounted so the LLM layer comes back clean; the project is
/// configured for ruff and the fake binary emits one finding. The gate
/// honours the tool's verdict without an allow-list.
#[tokio::test]
async fn tool_finding_blocks_with_no_fail_on() {
    let (dir, server) = setup_mock(r#"{"issues": []}"#).await;
    crate::test_support::write_drep_toml(dir.path(), &format!("{}/v1", server.uri()));

    // Fake `ruff` that emits one finding. We use `printf '%s'` (no trailing
    // newline) so the JSON parser sees exactly one element.
    let bin = dir.path().join("venv/bin/ruff");
    std::fs::create_dir_all(bin.parent().unwrap()).expect("bin dir");
    write_executable(
        &bin,
        "#!/bin/sh\nprintf '%s' '[{\"code\":\"F401\",\"filename\":\"src/lib.py\",\"location\":{\"row\":1,\"column\":1},\"message\":\"unused import\"}]'\n",
    );
    std::fs::write(dir.path().join("pyproject.toml"), "").expect("pyproject");
    std::fs::create_dir_all(dir.path().join("src")).expect("src dir");
    std::fs::write(dir.path().join("src/lib.py"), "import os\n").expect("lib.py");

    let exit = run_paths(dir.path().join("src/lib.py"), dir.path()).await;
    assert_eq!(
        exit,
        check::Exit::FoundIssues,
        "a tool finding with no --fail-on must return FoundIssues"
    );
    assert_eq!(exit.code(), 1, "FoundIssues must map to exit 1");
}

// ---------- 14 (subprocess) ----------

/// Criterion 14: an LLM finding at severity `error` does **not** block
/// with no `--fail-on`, but it still appears in the rendered output.
///
/// Asserting only the exit would also hold for an implementation that
/// silently drops LLM findings; asserting only the output would also hold
/// for one that classifies them as blocking. Both halves in one test is
/// what makes "inform, not gate" observable end-to-end.
#[test]
fn llm_finding_does_not_block_without_fail_on_but_renders() {
    // Subprocess so the test can capture stdout. The mock is mounted on a
    // wiremock server inside this process; the spawned binary hits it
    // over HTTP exactly as production would.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("runtime");

    let (dir, _server) = runtime.block_on(async {
        let dir = tempfile::tempdir().expect("tempdir");
        let server = MockServer::start().await;
        let body = r#"{"issues":[{"line":1,"severity":"critical","category":"bug","message":"something smells"}]}"#;
        mount_sse(
            &server,
            wiremock::ResponseTemplate::new(200)
                .set_body_raw(sse(&[body]), "text/event-stream"),
        )
        .await;
        crate::test_support::write_drep_toml(dir.path(), &format!("{}/v1", server.uri()));
        std::fs::write(dir.path().join("lib.py"), "x = 1\n").expect("lib.py");
        (dir, server)
    });

    let output = run_drep(dir.path(), &["check", "lib.py"]);
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert_eq!(
        output.status.code(),
        Some(check::Exit::Clean.code() as i32),
        "no --fail-on means LLM findings inform, not block; stderr was {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        stdout.contains("something smells"),
        "the LLM finding must appear in rendered output, got {stdout:?}"
    );
    // We don't assert the exact format here - that's criterion 19's job.
    // We only assert the message reaches the user.
}

// ---------- 15 ----------

/// Criterion 15: the same LLM finding at severity `error` blocks under
/// `--fail-on error`.
///
/// Pairs with criterion 14: the only thing that changes between the two
/// is the flag, and the gate must respond to it.
#[tokio::test]
async fn llm_finding_at_error_blocks_under_fail_on_error() {
    let (dir, server) = setup_mock(
        r#"{"issues":[{"line":1,"severity":"critical","category":"bug","message":"bad"}]}"#,
    )
    .await;
    crate::test_support::write_drep_toml(dir.path(), &format!("{}/v1", server.uri()));
    std::fs::write(dir.path().join("lib.py"), "x = 1\n").expect("lib.py");

    let args = CheckArgs {
        paths: vec![dir.path().join("lib.py")],
        staged: false,
        diff: None,
        tip: None,
        format: OutputFormat::Text,
        fail_on: Some(Severity::Error),
    };
    let exit = run_paths_with(&args, dir.path()).await;
    assert_eq!(
        exit,
        check::Exit::FoundIssues,
        "an error-level LLM finding under --fail-on error must block"
    );
}

// ---------- 16 ----------

/// Criterion 16: an LLM finding at severity `warning` does not block
/// under `--fail-on error`.
///
/// `warning < error`, so it is below the threshold and stays
/// informational. A gate that admitted anything at or above `error` (i.e.,
/// also admitted `warning`) would fail this test.
#[tokio::test]
async fn llm_finding_at_warning_does_not_block_under_fail_on_error() {
    let (dir, server) = setup_mock(
        r#"{"issues":[{"line":1,"severity":"medium","category":"style","message":"meh"}]}"#,
    )
    .await;
    crate::test_support::write_drep_toml(dir.path(), &format!("{}/v1", server.uri()));
    std::fs::write(dir.path().join("lib.py"), "x = 1\n").expect("lib.py");

    let args = CheckArgs {
        paths: vec![dir.path().join("lib.py")],
        staged: false,
        diff: None,
        tip: None,
        format: OutputFormat::Text,
        fail_on: Some(Severity::Error),
    };
    let exit = run_paths_with(&args, dir.path()).await;
    assert_eq!(
        exit,
        check::Exit::Clean,
        "warning under --fail-on error must not block"
    );
}

// ---------- 17 ----------

/// Criterion 17: a failure outranks a finding.
///
/// Constructing the precedence test from inside `check::run` is awkward:
/// the orchestrator's first-wins rule means the failure axis only
/// appears when reading failed. The cleanest expression of the
/// precedence is to call `gate` directly with both populated. `gate` is
/// not exported, so this test goes through `check::run` with a file that
/// the input layer will refuse to read AND a tool that emits a finding.
/// The exit must be `Unanalyzed` (2), not `FoundIssues` (1).
#[tokio::test]
async fn failure_outranks_finding() {
    let (dir, server) = setup_mock(r#"{"issues": []}"#).await;
    crate::test_support::write_drep_toml(dir.path(), &format!("{}/v1", server.uri()));

    // One file produces a tool finding.
    let bin = dir.path().join("venv/bin/ruff");
    std::fs::create_dir_all(bin.parent().unwrap()).expect("bin dir");
    write_executable(
        &bin,
        "#!/bin/sh\nprintf '%s' '[{\"code\":\"F401\",\"filename\":\"src/lib.py\",\"location\":{\"row\":1,\"column\":1},\"message\":\"unused import\"}]'\n",
    );
    std::fs::write(dir.path().join("pyproject.toml"), "").expect("pyproject");
    std::fs::create_dir_all(dir.path().join("src")).expect("src dir");
    std::fs::write(dir.path().join("src/lib.py"), "import os\n").expect("lib.py");
    // The second file is non-UTF-8: a binary blob. The input layer
    // rejects it before either the deterministic or the LLM layer can
    // touch it, and the orchestrator records it as a read failure.
    std::fs::write(dir.path().join("src/bad.py"), [0xFFu8, 0xFE]).expect("bad.py");

    let args = CheckArgs {
        paths: vec![dir.path().join("src/lib.py"), dir.path().join("src/bad.py")],
        staged: false,
        diff: None,
        tip: None,
        format: OutputFormat::Text,
        fail_on: None,
    };
    let exit = run_paths_with(&args, dir.path()).await;
    assert_eq!(
        exit,
        check::Exit::Unanalyzed,
        "a run with a blocking tool finding AND an unanalyzed file must \
         exit 2, not 1"
    );
    assert_eq!(exit.code(), 2, "Unanalyzed must map to exit 2");
}

// ---------- 18 (subprocess) ----------

/// Criterion 18: a run whose only problem is an unreachable LLM endpoint
/// exits 2, and its text output does not contain `No issues found.`
///
/// "No issues found." would imply a clean pass; the run is not clean.
/// Asserting both halves together makes it impossible to satisfy the
/// test by reporting `Unanalyzed` while still printing the misleading
/// clean line - or by suppressing the clean line while silently exiting
/// 0.
#[test]
fn unreachable_endpoint_exits_2_and_does_not_print_clean() {
    // Use a port that nothing is listening on. 1 is privileged and the OS
    // rejects the connect immediately, which keeps the test fast.
    let dir = tempfile::tempdir().expect("tempdir");
    crate::test_support::write_drep_toml(dir.path(), "http://127.0.0.1:1/v1");
    std::fs::write(dir.path().join("lib.py"), "x = 1\n").expect("lib.py");

    let output = run_drep(dir.path(), &["check", "lib.py"]);
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert_eq!(
        output.status.code(),
        Some(check::Exit::Unanalyzed.code() as i32),
        "an unreachable endpoint must exit 2, stderr was {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !stdout.contains("No issues found."),
        "an unanalyzed run must not print 'No issues found.', got {stdout:?}"
    );
}

// ---------- shared helpers ----------
