//! Gating and exit codes: criteria 12-18.
//!
//! Criteria 12-18 plus cache-limit and push-handshake regressions share one
//! precedence table:
//!
//! - Exit 0 (`Clean`): no failures, no blocking findings.
//! - Exit 1 (`FoundIssues`): tool findings always block; LLM findings block
//!   when `--fail-on` admits their severity and is set.
//! - Exit 2 (`Unanalyzed`): any non-cache failure outranks any finding.
//! - Exit 3 (`CacheMiss`): cache-only review found uncached files.
//!
//! Tests 12-17 are in-process: they call `check::run` directly and assert
//! only on the returned `Exit`. Tests 14 and 18 also assert on the
//! rendered output (a finding must be visible in text output, and an
//! unreachable endpoint must not print "No issues found."), so they
//! spawn the drep binary as a subprocess to capture stdout.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use wiremock::MockServer;

use super::support::run_drep;
use crate::analysis::findings::{Finding, Severity};
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
        pre_commit_push: false,
        format: OutputFormat::Text,
        fail_on,
        cache_only: false,
        push_gate: false,
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

/// Configure ruff and install a local stub that emits one F401 finding.
fn install_fake_ruff(dir: &Path) {
    let bin = dir.join("venv/bin/ruff");
    std::fs::create_dir_all(bin.parent().expect("ruff parent")).expect("bin dir");
    write_executable(
        &bin,
        "#!/bin/sh\nprintf '%s' '[{\"code\":\"F401\",\"filename\":\"src/lib.py\",\"location\":{\"row\":1,\"column\":1},\"message\":\"unused import\"}]'\n",
    );
    std::fs::write(dir.join("pyproject.toml"), "").expect("pyproject");
    std::fs::create_dir_all(dir.join("src")).expect("src dir");
    std::fs::write(dir.join("src/lib.py"), "import os\n").expect("lib.py");
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

/// A cache-only run must never contact the provider. Its distinct exit code is
/// what lets the pre-push hook warm the cache, stop the current push, and ask
/// Git to reconnect for the fast retry instead of resuming a stale transport.
#[tokio::test]
async fn cache_only_miss_exits_3_without_contacting_the_provider() {
    let (dir, server) = setup_mock(r#"{"issues": []}"#).await;
    crate::test_support::write_drep_toml(dir.path(), &format!("{}/v1", server.uri()));
    let source = dir.path().join("lib.py");
    std::fs::write(&source, "x = 1\n").expect("lib.py");
    let mut args = args(vec![source], None);
    args.cache_only = true;

    let exit = run_paths_with(&args, dir.path()).await;

    assert_eq!(exit, check::Exit::CacheMiss);
    assert_eq!(exit.code(), 3);
    assert_eq!(crate::test_support::request_count(&server).await, 0);
}

/// Push-gate mode performs the cold review, deliberately exits 3 before Git
/// resumes its old connection, then lets the immediate cached retry through.
#[tokio::test]
async fn push_gate_warms_once_then_the_cached_retry_is_clean() {
    let (dir, server) = setup_mock(r#"{"issues": []}"#).await;
    crate::test_support::write_drep_toml(dir.path(), &format!("{}/v1", server.uri()));
    let source = dir.path().join("lib.py");
    std::fs::write(&source, "x = 1\n").expect("lib.py");
    let cache = Cache::new(dir.path().join("push-cache"), 30, 8 * 1024 * 1024);
    let mut args = args(vec![source], None);
    args.push_gate = true;

    let first = check::run_with(&args, dir.path(), cache.clone())
        .await
        .expect("cold push gate");
    let second = check::run_with(&args, dir.path(), cache)
        .await
        .expect("warm push gate");

    assert_eq!(first, check::Exit::CacheMiss);
    assert_eq!(second, check::Exit::Clean);
    assert_eq!(crate::test_support::request_count(&server).await, 1);
}

/// Exit 3 alone cannot distinguish a successful warm from an internal cache
/// miss that accidentally survived the live pass. The real binary must print
/// the reconnect instruction and must not render that stale marker as failed
/// analysis.
#[tokio::test(flavor = "multi_thread")]
async fn a_successful_push_warm_renders_only_the_reconnect_instruction() {
    let (dir, server) = setup_mock(r#"{"issues": []}"#).await;
    crate::test_support::write_drep_toml(dir.path(), &format!("{}/v1", server.uri()));
    std::fs::write(dir.path().join("lib.py"), "x = 1\n").expect("lib.py");

    let output = run_drep(dir.path(), &["check", "--push-gate", "lib.py"]);
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert_eq!(output.status.code(), Some(3), "stdout: {stdout}");
    assert!(stdout.contains("Run git push again"), "stdout: {stdout}");
    assert!(
        !stdout.contains("could not be analyzed"),
        "stdout: {stdout}"
    );
    assert!(!stdout.contains("not cached"), "stdout: {stdout}");
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

    install_fake_ruff(dir.path());

    let exit = run_paths(dir.path().join("src/lib.py"), dir.path()).await;
    assert_eq!(
        exit,
        check::Exit::FoundIssues,
        "a tool finding with no --fail-on must return FoundIssues"
    );
    assert_eq!(exit.code(), 1, "FoundIssues must map to exit 1");
}

/// A cache-only miss is operationally softer than a deterministic finding:
/// retrying cannot make the project's own tool finding disappear.
#[tokio::test]
async fn tool_findings_outrank_cache_only_misses() {
    let (dir, server) = setup_mock(r#"{"issues": []}"#).await;
    crate::test_support::write_drep_toml(dir.path(), &format!("{}/v1", server.uri()));
    install_fake_ruff(dir.path());
    let mut check_args = args(vec![dir.path().join("src/lib.py")], None);
    check_args.cache_only = true;

    let exit = run_paths_with(&check_args, dir.path()).await;

    assert_eq!(exit, check::Exit::FoundIssues);
    assert_eq!(crate::test_support::request_count(&server).await, 0);
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

    let args = args(vec![dir.path().join("lib.py")], Some(Severity::Error));
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

    let args = args(vec![dir.path().join("lib.py")], Some(Severity::Error));
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
    install_fake_ruff(dir.path());
    // The second file is non-UTF-8: a binary blob. The input layer
    // rejects it before either the deterministic or the LLM layer can
    // touch it, and the orchestrator records it as a read failure.
    std::fs::write(dir.path().join("src/bad.py"), [0xFFu8, 0xFE]).expect("bad.py");

    let args = args(
        vec![dir.path().join("src/lib.py"), dir.path().join("src/bad.py")],
        None,
    );
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
    // Ask the OS for an unused loopback port, then close it before drep runs.
    // Removing proxy variables in `run_drep` keeps this request local.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let port = listener.local_addr().expect("local address").port();
    drop(listener);
    let dir = tempfile::tempdir().expect("tempdir");
    crate::test_support::write_drep_toml(dir.path(), &format!("http://127.0.0.1:{port}/v1"));
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

#[test]
fn successful_compilation_suppresses_only_explicit_compile_failure_claims() {
    let finding = |path: &str, compile_failure: bool| Finding {
        kind: "bug".to_owned(),
        severity: Severity::Error,
        file_path: path.to_owned(),
        line: 1,
        column: None,
        message: "review".to_owned(),
        suggestion: None,
        asserts_compile_failure: compile_failure,
        fingerprint: None,
    };
    let mut findings = vec![
        finding("src/compiled.rs", true),
        finding("src/compiled.rs", false),
        finding("src/unchecked.rs", true),
    ];
    let compiled = BTreeSet::from([PathBuf::from("src/compiled.rs")]);

    check::suppress_disproved_compile_claims(&mut findings, &compiled);

    assert_eq!(findings.len(), 2);
    assert!(
        findings
            .iter()
            .any(|finding| !finding.asserts_compile_failure)
    );
    assert!(
        findings
            .iter()
            .any(|finding| finding.file_path == "src/unchecked.rs")
    );
}

// ---------- shared helpers ----------
