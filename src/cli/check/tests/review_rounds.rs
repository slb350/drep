//! End-to-end contracts for the bounded semantic-review cycle.

use std::path::{Path, PathBuf};

use wiremock::MockServer;

use super::super::review_budget::{Budget, Claim};
use super::support::{check_args as args, run_drep};
use crate::analysis::acknowledgements::Store;
use crate::analysis::findings::{Finding, Severity};
use crate::cli::check;
use crate::diff::hunks::Hunk;
use crate::llm::cache::Cache;
use crate::test_support::{
    git_commit_all as commit_all, git_output, server_returning, write_executable,
};

async fn setup_mock(body: &str) -> (tempfile::TempDir, MockServer) {
    let dir = tempfile::tempdir().expect("tempdir");
    let server = server_returning(&[body]).await;
    (dir, server)
}

fn git_head(dir: &Path) -> String {
    git_output(dir, &["rev-parse", "HEAD"])
}

async fn commit_rounds(root: &Path, rounds: u32) {
    let budget = Budget::for_repo(root, 3).await.expect("budget");
    for _ in 0..rounds {
        let Claim::Reserved(claim) = budget.claim().expect("claim") else {
            panic!("round must be available");
        };
        claim.commit().expect("commit round");
    }
}

#[test]
fn fourth_fresh_staged_review_is_blocked_but_cache_and_overrides_remain_available() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let (dir, server) = runtime.block_on(async {
        let dir = tempfile::tempdir().expect("tempdir");
        crate::test_support::git_init(dir.path());
        let server = server_returning(&[
            r#"{"issues":[{"line":1,"severity":"high","category":"bug","message":"fix me"}]}"#,
        ])
        .await;
        crate::test_support::write_drep_toml(dir.path(), &format!("{}/v1", server.uri()));
        std::fs::write(dir.path().join("lib.py"), "x = 0\n").expect("lib.py");
        commit_all(dir.path(), "base");
        (dir, server)
    });

    for round in 1..=3 {
        std::fs::write(dir.path().join("lib.py"), format!("x = {round}\n")).expect("lib.py");
        crate::test_support::git_add(dir.path(), "lib.py");
        let output = run_drep(dir.path(), &["check", "--staged"]);
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert_eq!(output.status.code(), Some(0), "stdout: {stdout}");
        assert!(
            stdout.contains(&format!("Fresh LLM review round {round} of 3")),
            "stdout: {stdout}"
        );
    }

    std::fs::write(dir.path().join("lib.py"), "x = 4\n").expect("lib.py");
    crate::test_support::git_add(dir.path(), "lib.py");
    let blocked = run_drep(dir.path(), &["check", "--staged"]);
    let blocked_stdout = String::from_utf8_lossy(&blocked.stdout);
    assert_eq!(blocked.status.code(), Some(2), "stdout: {blocked_stdout}");
    assert!(
        blocked_stdout.contains("fresh LLM review limit reached (3 of 3)"),
        "stdout: {blocked_stdout}"
    );
    assert_eq!(
        runtime.block_on(crate::test_support::request_count(&server)),
        3
    );

    // The cap blocks only a cold fourth review. Restoring already-reviewed
    // content reuses its cached verdict without another provider request.
    std::fs::write(dir.path().join("lib.py"), "x = 3\n").expect("lib.py");
    crate::test_support::git_add(dir.path(), "lib.py");
    let cached = run_drep(dir.path(), &["check", "--staged"]);
    assert_eq!(cached.status.code(), Some(0));
    assert_eq!(
        runtime.block_on(crate::test_support::request_count(&server)),
        3
    );

    std::fs::write(dir.path().join("lib.py"), "x = 4\n").expect("lib.py");
    crate::test_support::git_add(dir.path(), "lib.py");
    let extended = run_drep(
        dir.path(),
        &["check", "--staged", "--max-review-rounds", "4"],
    );
    let extended_stdout = String::from_utf8_lossy(&extended.stdout);
    assert_eq!(extended.status.code(), Some(0), "stdout: {extended_stdout}");
    assert!(
        extended_stdout.contains("Fresh LLM review round 4 of 4"),
        "stdout: {extended_stdout}"
    );
    assert_eq!(
        runtime.block_on(crate::test_support::request_count(&server)),
        4
    );

    std::fs::write(dir.path().join("lib.py"), "x = 5\n").expect("lib.py");
    crate::test_support::git_add(dir.path(), "lib.py");
    let unlimited = run_drep(dir.path(), &["check", "--staged", "--unlimited-reviews"]);
    let unlimited_stdout = String::from_utf8_lossy(&unlimited.stdout);
    assert_eq!(
        unlimited.status.code(),
        Some(0),
        "stdout: {unlimited_stdout}"
    );
    assert!(
        unlimited_stdout.contains("Fresh LLM review ran with no round limit"),
        "stdout: {unlimited_stdout}"
    );
    assert_eq!(
        runtime.block_on(crate::test_support::request_count(&server)),
        5
    );
}

#[tokio::test]
async fn clean_push_gate_warm_resets_the_review_cycle() {
    let (dir, server) = setup_mock(r#"{"issues": []}"#).await;
    crate::test_support::git_init(dir.path());
    crate::test_support::write_drep_toml(dir.path(), &format!("{}/v1", server.uri()));
    std::fs::write(dir.path().join("lib.py"), "x = 0\n").expect("lib.py");
    commit_all(dir.path(), "base");
    let base = git_head(dir.path());
    std::fs::write(dir.path().join("lib.py"), "x = 1\n").expect("lib.py");
    commit_all(dir.path(), "change one");

    commit_rounds(dir.path(), 3).await;

    let cache = Cache::new(dir.path().join("reset-cache"), 30, 8 * 1024 * 1024);
    let mut check_args = args(Vec::new(), None);
    check_args.diff = Some(base.clone());
    check_args.push_gate = true;
    check_args.max_review_rounds = Some(4);
    let first = check::run_with(&check_args, dir.path(), cache.clone())
        .await
        .expect("extended clean warm");
    assert_eq!(first, check::Exit::CacheMiss);

    std::fs::write(dir.path().join("lib.py"), "x = 2\n").expect("lib.py");
    commit_all(dir.path(), "change two");
    check_args.max_review_rounds = None;
    let second = check::run_with(&check_args, dir.path(), cache)
        .await
        .expect("default budget after reset");

    assert_eq!(second, check::Exit::CacheMiss);
    assert_eq!(crate::test_support::request_count(&server).await, 2);
}

#[tokio::test]
async fn a_clean_named_path_check_does_not_reset_an_authoritative_cycle() {
    let (dir, server) = setup_mock(r#"{"issues": []}"#).await;
    crate::test_support::git_init(dir.path());
    crate::test_support::write_drep_toml(dir.path(), &format!("{}/v1", server.uri()));
    std::fs::write(dir.path().join("lib.py"), "x = 0\n").expect("lib.py");
    commit_all(dir.path(), "base");

    commit_rounds(dir.path(), 3).await;

    let cache = Cache::new(dir.path().join("named-cache"), 30, 8 * 1024 * 1024);
    let named = check::run_with(
        &args(vec![dir.path().join("lib.py")], None),
        dir.path(),
        cache.clone(),
    )
    .await
    .expect("named check");
    assert_eq!(named, check::Exit::Clean);

    std::fs::write(dir.path().join("lib.py"), "x = 1\n").expect("lib.py");
    crate::test_support::git_add(dir.path(), "lib.py");
    let mut staged = args(Vec::new(), None);
    staged.staged = true;
    let blocked = check::run_with(&staged, dir.path(), cache)
        .await
        .expect("bounded staged check");

    assert_eq!(blocked, check::Exit::Unanalyzed);
    assert_eq!(crate::test_support::request_count(&server).await, 1);
}

#[tokio::test]
async fn a_clean_staged_subset_does_not_reset_the_full_change_cycle() {
    let (dir, server) = setup_mock(r#"{"issues": []}"#).await;
    crate::test_support::git_init(dir.path());
    crate::test_support::write_drep_toml(dir.path(), &format!("{}/v1", server.uri()));
    std::fs::write(dir.path().join("lib.py"), "x = 0\n").expect("lib.py");
    commit_all(dir.path(), "base");

    commit_rounds(dir.path(), 3).await;

    // The cached staged result is clean, but staged input can be only a subset
    // of the branch. It must not clear a cycle that the full diff has not yet
    // completed.
    std::fs::write(dir.path().join("lib.py"), "x = 1\n").expect("lib.py");
    crate::test_support::git_add(dir.path(), "lib.py");
    let mut staged = args(Vec::new(), None);
    staged.staged = true;
    staged.max_review_rounds = Some(4);
    let cache = Cache::new(dir.path().join("staged-cache"), 30, 8 * 1024 * 1024);
    let clean = check::run_with(&staged, dir.path(), cache.clone())
        .await
        .expect("extended staged review");
    assert_eq!(clean, check::Exit::Clean);

    std::fs::write(dir.path().join("lib.py"), "x = 2\n").expect("lib.py");
    crate::test_support::git_add(dir.path(), "lib.py");
    staged.max_review_rounds = None;
    let blocked = check::run_with(&staged, dir.path(), cache)
        .await
        .expect("default staged review");
    assert_eq!(blocked, check::Exit::Unanalyzed);
    assert_eq!(crate::test_support::request_count(&server).await, 1);
}

#[tokio::test]
async fn pure_analysis_failure_refunds_the_reserved_round() {
    let (dir, server) = setup_mock("this is not JSON").await;
    crate::test_support::git_init(dir.path());
    crate::test_support::write_drep_toml(dir.path(), &format!("{}/v1", server.uri()));
    std::fs::write(dir.path().join("lib.py"), "x = 0\n").expect("lib.py");
    commit_all(dir.path(), "base");
    std::fs::write(dir.path().join("lib.py"), "x = 1\n").expect("lib.py");
    crate::test_support::git_add(dir.path(), "lib.py");
    let mut staged = args(Vec::new(), None);
    staged.staged = true;

    let exit = check::run_with(
        &staged,
        dir.path(),
        Cache::new(dir.path().join("failure-cache"), 30, 8 * 1024 * 1024),
    )
    .await
    .expect("failed analysis still returns a verdict");
    assert_eq!(exit, check::Exit::Unanalyzed);

    let budget = Budget::for_repo(dir.path(), 3).await.expect("budget");
    let Claim::Reserved(claim) = budget.claim().expect("round refunded") else {
        panic!("pure failure must not consume a round");
    };
    assert_eq!(claim.round(), 1);
}

#[tokio::test]
async fn mixed_actionable_finding_and_analysis_failure_consumes_the_round() {
    let (dir, server) = setup_mock(
        r#"{"issues":[{"line":1,"severity":"high","category":"bug","message":"real"},{"line":1,"severity":"unknown","category":"bug","message":"bad record"}]}"#,
    )
    .await;
    crate::test_support::git_init(dir.path());
    crate::test_support::write_drep_toml(dir.path(), &format!("{}/v1", server.uri()));
    std::fs::write(dir.path().join("lib.py"), "x = 0\n").expect("lib.py");
    commit_all(dir.path(), "base");
    std::fs::write(dir.path().join("lib.py"), "x = 1\n").expect("lib.py");
    crate::test_support::git_add(dir.path(), "lib.py");
    let mut staged = args(Vec::new(), None);
    staged.staged = true;

    let exit = check::run_with(
        &staged,
        dir.path(),
        Cache::new(dir.path().join("mixed-cache"), 30, 8 * 1024 * 1024),
    )
    .await
    .expect("mixed result returns a verdict");
    assert_eq!(exit, check::Exit::Unanalyzed);

    let budget = Budget::for_repo(dir.path(), 3).await.expect("budget");
    let Claim::Reserved(claim) = budget.claim().expect("next round") else {
        panic!("two rounds should remain");
    };
    assert_eq!(claim.round(), 2);
}

#[tokio::test]
async fn an_acknowledged_live_finding_does_not_consume_a_round() {
    let (dir, server) = setup_mock(
        r#"{"issues":[{"line":1,"severity":"high","category":"bug","message":"known false positive"}]}"#,
    )
    .await;
    crate::test_support::git_init(dir.path());
    crate::test_support::write_drep_toml(dir.path(), &format!("{}/v1", server.uri()));
    std::fs::write(dir.path().join("lib.py"), "x = 0\n").expect("lib.py");
    commit_all(dir.path(), "base");
    std::fs::write(dir.path().join("lib.py"), "x = 1\n").expect("lib.py");
    crate::test_support::git_add(dir.path(), "lib.py");

    let mut candidate = vec![Finding {
        kind: "bug".to_owned(),
        severity: Severity::Error,
        file_path: "lib.py".to_owned(),
        line: 1,
        column: None,
        message: "known false positive".to_owned(),
        suggestion: None,
        asserts_compile_failure: false,
        fingerprint: None,
    }];
    let hunks = vec![vec![Hunk::whole_file(PathBuf::from("lib.py"), "x = 1\n")]];
    crate::analysis::acknowledgements::apply(&mut candidate, &hunks, &Store::default());
    let mut store = Store::default();
    store.insert(
        candidate[0]
            .fingerprint
            .clone()
            .expect("source-sensitive fingerprint"),
    );
    store.save(dir.path()).expect("save acknowledgement");

    let mut staged = args(Vec::new(), None);
    staged.staged = true;
    let exit = check::run_with(
        &staged,
        dir.path(),
        Cache::new(dir.path().join("acknowledged-cache"), 30, 8 * 1024 * 1024),
    )
    .await
    .expect("acknowledged review");

    assert_eq!(exit, check::Exit::Clean);
    assert_eq!(crate::test_support::request_count(&server).await, 1);
    let budget = Budget::for_repo(dir.path(), 3).await.expect("budget");
    let Claim::Reserved(claim) = budget.claim().expect("round one") else {
        panic!("the acknowledged finding must refund the reservation");
    };
    assert_eq!(claim.round(), 1);
}

#[tokio::test]
async fn a_compiler_disproved_live_finding_does_not_consume_a_round() {
    let (dir, server) = setup_mock(
        r#"{"issues":[{"line":1,"severity":"high","category":"bug","message":"this does not compile","compile_failure":true}]}"#,
    )
    .await;
    crate::test_support::git_init(dir.path());
    crate::test_support::write_drep_toml(dir.path(), &format!("{}/v1", server.uri()));
    std::fs::create_dir(dir.path().join("src")).expect("src directory");
    std::fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .expect("Cargo.toml");
    std::fs::write(
        dir.path().join("src/lib.rs"),
        "pub fn value() -> u8 { 0 }\n",
    )
    .expect("lib.rs");
    commit_all(dir.path(), "base");
    std::fs::write(
        dir.path().join("src/lib.rs"),
        "pub fn value() -> u8 { 1 }\n",
    )
    .expect("lib.rs");
    crate::test_support::git_add(dir.path(), "src/lib.rs");

    let mut staged = args(Vec::new(), None);
    staged.staged = true;
    let exit = check::run_with(
        &staged,
        dir.path(),
        Cache::new(dir.path().join("compile-cache"), 30, 8 * 1024 * 1024),
    )
    .await
    .expect("compiler-grounded review");

    assert_eq!(exit, check::Exit::Clean);
    assert_eq!(crate::test_support::request_count(&server).await, 1);
    let budget = Budget::for_repo(dir.path(), 3).await.expect("budget");
    let Claim::Reserved(claim) = budget.claim().expect("round one") else {
        panic!("the disproved finding must refund the reservation");
    };
    assert_eq!(claim.round(), 1);
}

#[tokio::test]
async fn deterministic_findings_prevent_a_full_diff_from_resetting_the_cycle() {
    let (dir, server) = setup_mock(r#"{"issues": []}"#).await;
    crate::test_support::git_init(dir.path());
    crate::test_support::write_drep_toml(dir.path(), &format!("{}/v1", server.uri()));
    std::fs::write(dir.path().join("pyproject.toml"), "").expect("pyproject.toml");
    std::fs::create_dir_all(dir.path().join("venv/bin")).expect("venv bin");
    write_executable(
        &dir.path().join("venv/bin/ruff"),
        "#!/bin/sh\nprintf '%s' '[{\"code\":\"F401\",\"filename\":\"lib.py\",\"location\":{\"row\":1,\"column\":1},\"message\":\"unused import\"}]'\n",
    );
    std::fs::write(dir.path().join("lib.py"), "x = 0\n").expect("lib.py");
    commit_all(dir.path(), "base");
    let base = git_head(dir.path());
    std::fs::write(dir.path().join("lib.py"), "x = 1\n").expect("lib.py");
    commit_all(dir.path(), "change one");

    commit_rounds(dir.path(), 3).await;

    let cache = Cache::new(dir.path().join("tool-cache"), 30, 8 * 1024 * 1024);
    let mut diff = args(Vec::new(), None);
    diff.diff = Some(base.clone());
    diff.max_review_rounds = Some(4);
    let with_tool_finding = check::run_with(&diff, dir.path(), cache.clone())
        .await
        .expect("extended full diff");
    assert_eq!(with_tool_finding, check::Exit::FoundIssues);

    std::fs::write(dir.path().join("lib.py"), "x = 2\n").expect("lib.py");
    commit_all(dir.path(), "change two");
    diff.max_review_rounds = None;
    let still_bounded = check::run_with(&diff, dir.path(), cache)
        .await
        .expect("default full diff");

    assert_eq!(still_bounded, check::Exit::Unanalyzed);
    assert_eq!(crate::test_support::request_count(&server).await, 1);
}
