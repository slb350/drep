//! Refusal against the modes that have their own path to a verdict.
//!
//! Each of these reaches the semantic layer differently - a cached verdict served
//! without a request, the push gate's warm-and-reconnect handshake, the bounded
//! review budget's authoritative accounting - and each is a separate way for a
//! refusal wired only into the live pass to be bypassed. `--cache-only` is the
//! likeliest: nothing is sent, so it reads as harmless, and it still hands a marked
//! repository a model's verdict on its source.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::super::review_budget::{Budget, Claim};
use super::marker_refusal::{MARKER, repo};
use super::support::{check_args, run_drep_with_site};
use crate::cli::MachineFiles;
use crate::cli::check;
use crate::llm::cache::Cache;
use crate::test_support::{git_add, request_count};

/// A cache rooted under `dir`, so two runs in one test share entries.
fn cache_in(dir: &Path) -> Cache {
    Cache::new(dir.join("test-cache"), 30, 8 * 1024 * 1024)
}

/// Run a paths-mode check, optionally cache-only, against a named policy file.
async fn check_in(dir: &Path, site: &Path, cache_only: bool) -> anyhow::Result<check::Exit> {
    let mut args = check_args(vec![dir.join("lib.py")], None);
    args.cache_only = cache_only;
    check::run_against(
        &args,
        dir,
        cache_in(dir),
        &MachineFiles {
            auth: &dir.join("auth.toml"),
            policy: site,
        },
    )
    .await
}

/// A cached verdict is a model's verdict, so it is not served either.
///
/// Warm the entry while the repository is unmarked, then mark it and ask again in
/// the mode that answers purely from cache. A refusal wired into the live pass
/// alone passes every other test in this suite and fails here.
#[tokio::test]
async fn a_cached_verdict_is_not_served_for_a_refused_repository() {
    let (dir, server, site) = repo(&[MARKER]).await;

    let warm = check_in(dir.path(), &site, false)
        .await
        .expect("warming the cache");
    assert_eq!(warm, check::Exit::Clean);
    assert_eq!(request_count(&server).await, 1);

    std::fs::write(dir.path().join(MARKER), "").expect("marker");
    let refused = check_in(dir.path(), &site, true)
        .await
        .expect("a refusal is a verdict, not a crash");

    assert_eq!(
        refused,
        check::Exit::Unanalyzed,
        "the cached answer came from a model, and this repository's source is \
         not allowed to have reached one"
    );
    assert_eq!(request_count(&server).await, 1);
}

/// The push gate never turns a refusal into the exit-3 reconnect handshake.
///
/// Exit 3 means "the missing review completed and is cached, push again". Printing
/// that for a repository whose review will never happen sends the developer round
/// a loop that cannot terminate.
#[tokio::test]
async fn push_gate_in_a_refused_repository_never_asks_for_a_reconnect() {
    let (dir, server, site) = repo(&[MARKER]).await;
    std::fs::write(dir.path().join(MARKER), "").expect("marker");
    git_add(dir.path(), "lib.py");

    let output = run_drep_with_site(
        dir.path(),
        &site,
        &["check", "--push-gate", "--staged", "--format", "json"],
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");

    assert_eq!(output.status.code(), Some(2), "stdout: {stdout}");
    assert_eq!(parsed["exit"], 2, "stdout: {stdout}");
    assert_eq!(parsed["retry_push"], false, "stdout: {stdout}");
    assert!(!stdout.contains("Run git push again"), "stdout: {stdout}");
    assert_eq!(request_count(&server).await, 0, "stdout: {stdout}");
}

/// A machine consumer is told the review did not happen, not handed prose.
///
/// `unanalyzed` exists so a pipeline can tell a clean run from one that never ran;
/// a text-only refusal leaves it reading an empty array beside exit 2.
#[tokio::test]
async fn json_output_reports_the_refusal_as_unanalyzed() {
    let (dir, server, site) = repo(&[MARKER]).await;
    std::fs::write(dir.path().join(MARKER), "").expect("marker");

    let output = run_drep_with_site(dir.path(), &site, &["check", "lib.py", "--format", "json"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");

    assert_eq!(parsed["exit"], 2, "stdout: {stdout}");
    assert_eq!(parsed["findings"].as_array().map(Vec::len), Some(0));
    let unanalyzed = parsed["unanalyzed"]
        .as_array()
        .expect("unanalyzed is an array");
    assert_eq!(unanalyzed.len(), 1, "stdout: {stdout}");
    assert_eq!(
        unanalyzed[0]["kind"], "site_policy_refused",
        "a pipeline deciding whether it was refused must not pattern-match \
         English; stdout: {stdout}"
    );
    assert!(
        unanalyzed[0]["marker"]
            .as_str()
            .is_some_and(|marker| marker.ends_with(MARKER)),
        "stdout: {stdout}"
    );
    assert_eq!(
        unanalyzed[0]["policy"],
        serde_json::json!(site.to_string_lossy()),
        "stdout: {stdout}"
    );
    assert!(
        parsed["providers"].as_array().is_some_and(Vec::is_empty),
        "the report answers who reviewed this code, and nobody did; \
         stdout: {stdout}"
    );
    assert_eq!(request_count(&server).await, 0, "stdout: {stdout}");
}

/// One entry per file drep was asked about, not one per run.
///
/// Every other refusal test uses a single-file work set, so "report the refusal"
/// and "report it for each file" were indistinguishable. A refusal that recorded
/// only the first file still exits 2, so no exit assertion notices - while a
/// consumer reconciling the files it asked about against `unanalyzed` reads the
/// rest as reviewed and clean.
#[tokio::test]
async fn every_file_in_a_refused_work_set_is_reported_unanalyzed() {
    let (dir, server, site) = repo(&[MARKER]).await;
    std::fs::write(dir.path().join(MARKER), "").expect("marker");
    for name in ["second.py", "third.py"] {
        std::fs::write(dir.path().join(name), "x = 1\n").expect("source");
    }

    let output = run_drep_with_site(
        dir.path(),
        &site,
        &[
            "check",
            "lib.py",
            "second.py",
            "third.py",
            "--format",
            "json",
        ],
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");

    let unanalyzed = parsed["unanalyzed"]
        .as_array()
        .expect("unanalyzed is an array");
    assert_eq!(unanalyzed.len(), 3, "stdout: {stdout}");
    assert!(
        unanalyzed
            .iter()
            .all(|entry| entry["kind"] == "site_policy_refused"),
        "each one for the same reason, and the reason is the policy; stdout: {stdout}"
    );
    assert_eq!(request_count(&server).await, 0, "stdout: {stdout}");
}

/// A refused run neither reserves a review round nor resets the cycle.
///
/// Two committed rounds, then an authoritative `--staged` check. A stray `claim()`
/// would add a slot, and the clean-reset guard firing - which it would if the
/// refusal never reached `failed_files` - would remove both.
#[tokio::test]
async fn a_refused_run_claims_no_review_round_and_resets_no_cycle() {
    let (dir, server, site) = repo(&[MARKER]).await;
    let budget = Budget::for_repo(dir.path(), 3).await.expect("budget");
    for _ in 0..2 {
        let Claim::Reserved(claim) = budget.claim().expect("claim") else {
            panic!("a round must be available");
        };
        claim.commit().expect("commit the round");
    }
    let cycles = dir.path().join(".git/drep");
    let before = snapshot(&cycles);
    assert!(!before.is_empty(), "the fixture must have written slots");

    std::fs::write(dir.path().join(MARKER), "").expect("marker");
    git_add(dir.path(), "lib.py");
    let mut args = check_args(Vec::new(), None);
    args.staged = true;
    let exit = check::run_against(
        &args,
        dir.path(),
        cache_in(dir.path()),
        &MachineFiles {
            auth: &dir.path().join("auth.toml"),
            policy: &site,
        },
    )
    .await
    .expect("a refusal is a verdict, not a crash");

    assert_eq!(exit, check::Exit::Unanalyzed);
    assert_eq!(
        snapshot(&cycles),
        before,
        "a review that never happened neither consumes a round nor completes a \
         cycle, so the quota state is untouched byte for byte"
    );
    assert_eq!(request_count(&server).await, 0);
}

/// Nothing staged is clean, even under a refusal.
///
/// The refusal reports the files drep was asked about; asked about none, it owes
/// no review and has nothing to report. A blanket run-level failure would block
/// every commit that touches only, say, a README.
#[tokio::test]
async fn an_empty_work_set_in_a_refused_repository_is_clean() {
    let (dir, server, site) = repo(&[MARKER]).await;
    std::fs::write(dir.path().join(MARKER), "").expect("marker");

    let output = run_drep_with_site(dir.path(), &site, &["check", "--staged"]);
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert_eq!(output.status.code(), Some(0), "stdout: {stdout}");
    assert!(stdout.contains("No issues found."), "stdout: {stdout}");
    assert_eq!(request_count(&server).await, 0, "stdout: {stdout}");
}

/// Every regular file under `root`, keyed by path relative to it.
///
/// Contents, not just names: a slot rewritten in place with a different owner
/// token is the same filename and a different authorization.
fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut found = BTreeMap::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory)
            .into_iter()
            .flatten()
            .flatten()
        {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if let Ok(bytes) = std::fs::read(&path) {
                let relative = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
                found.insert(relative, bytes);
            }
        }
    }
    found
}
