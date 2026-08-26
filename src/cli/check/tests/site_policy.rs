//! The machine-level policy layer inside the orchestrator.
//!
//! One property, three angles: a policy file that exists and cannot be loaded
//! stops the run, and stops it *first*. The unconstrained run that would
//! otherwise follow reports as compliance - nothing in its output says the
//! policy was never applied - which is the same defect one level up from a check
//! that did not run being reported as a pass.

use std::path::Path;

use super::support::check_args as args;
use crate::cli::check;
use crate::llm::cache::Cache;
use crate::test_support::write_drep_toml;

/// A port nothing serves.
///
/// Every assertion here is about what happens before a request, so a live model
/// would only add latency and a way to fail for an unrelated reason.
const DEAD_ENDPOINT: &str = "http://127.0.0.1:59999/v1";

fn write_source(dir: &Path) -> std::path::PathBuf {
    let path = dir.join("lib.py");
    std::fs::write(&path, "x = 1\n").expect("lib.py");
    path
}

async fn run(
    dir: &Path,
    source: std::path::PathBuf,
    site_path: &Path,
) -> anyhow::Result<check::Exit> {
    check::run_against(
        &args(vec![source], None),
        dir,
        Cache::new(dir.join("test-cache"), 30, 8 * 1024 * 1024),
        &dir.join("auth.toml"),
        site_path,
    )
    .await
}

#[tokio::test]
async fn an_unparseable_site_policy_stops_the_check_before_anything_is_analyzed() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_drep_toml(dir.path(), DEAD_ENDPOINT);
    let source = write_source(dir.path());
    let site = dir.path().join("site.toml");
    std::fs::write(&site, "not toml at all\n").expect("site.toml");

    let err = run(dir.path(), source, &site)
        .await
        .expect_err("a policy that cannot be loaded is not enforced");

    let message = format!("{err:#}");
    assert!(
        message.contains(&site.display().to_string()),
        "the message has to name the policy file rather than fail later for an \
         unrelated reason such as transport; got {message}"
    );
}

/// Stops the test above from passing merely because the fixture cannot run at
/// all: with no policy file the same run reaches the dead endpoint and exits 2.
#[tokio::test]
async fn a_missing_site_policy_leaves_the_check_running_normally() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_drep_toml(dir.path(), DEAD_ENDPOINT);
    let source = write_source(dir.path());

    let exit = run(dir.path(), source, &dir.path().join("absent-site.toml"))
        .await
        .expect("a machine with no policy file is the ordinary case");

    assert_eq!(exit, check::Exit::Unanalyzed);
}

/// A broken policy must not be maskable by a broken repository config: the
/// policy is read first, so its failure is the one reported.
#[tokio::test]
async fn the_site_policy_is_read_before_the_repository_config() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("drep.toml"), "max_review_rounds = 2\n").expect("drep.toml");
    let source = write_source(dir.path());
    let site = dir.path().join("site.toml");
    std::fs::write(&site, "not toml at all\n").expect("site.toml");

    let err = run(dir.path(), source, &site)
        .await
        .expect_err("both files are broken, and the policy outranks the repository");

    let message = format!("{err:#}");
    assert!(
        message.contains(&site.display().to_string()),
        "got {message}"
    );
    assert!(
        !message.contains("[[llm]]"),
        "reporting the repository's missing provider first would let a broken \
         policy hide behind it; got {message}"
    );
}
