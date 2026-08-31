//! The machine-level policy layer inside the orchestrator.
//!
//! One property, three angles: a policy file that exists and cannot be loaded
//! stops the run, and stops it *first*. The unconstrained run that would
//! otherwise follow reports as compliance - nothing in its output says the
//! policy was never applied - which is the same defect one level up from a check
//! that did not run being reported as a pass.

use std::path::Path;

use super::support::check_args as args;
use crate::cli::MachineFiles;
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
        &MachineFiles {
            auth: &dir.join("auth.toml"),
            policy: site_path,
        },
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
    let endpoint = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("POST"))
        .respond_with(wiremock::ResponseTemplate::new(503))
        .mount(&endpoint)
        .await;
    write_drep_toml(dir.path(), &format!("{}/v1", endpoint.uri()));
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

/// The ceiling reaches the config the run will actually use.
///
/// Every other ceiling test calls `SiteConfig::apply` or `clamp_concurrency`
/// directly, so deleting the call from the orchestrator left the whole suite green
/// while `max_concurrent_ceiling` constrained nothing - and `doctor`, which
/// computes its note from the raw TOML tree, went on printing "lowered to 4" for a
/// clamp that no longer happened. A documented policy that is a no-op and reports
/// as enforced is the silent pass this layer exists to refuse.
#[test]
fn the_site_ceiling_lowers_the_config_the_run_will_use() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("drep.toml"),
        "[[llm]]\nendpoint = \"http://e/v1\"\nmodel = \"m\"\nmax_concurrent = 8\n",
    )
    .expect("drep.toml");
    let policy = dir.path().join("site.toml");
    std::fs::write(&policy, "max_concurrent_ceiling = 2\n").expect("site.toml");
    let site = crate::config::site::load(&policy)
        .expect("the policy loads")
        .expect("the policy is present");

    let (config_path, ceilinged) =
        check::configured(dir.path(), Some(&site)).expect("the config loads");
    let (_, unconstrained) = check::configured(dir.path(), None).expect("the config loads");

    assert_eq!(config_path, dir.path().join("drep.toml"));
    assert_eq!(ceilinged.llm[0].max_concurrent, 2);
    assert_eq!(
        unconstrained.llm[0].max_concurrent, 8,
        "the fixture has to arrive above the ceiling for the clamp to mean anything"
    );
}
