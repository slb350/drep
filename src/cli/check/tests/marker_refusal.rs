//! Marker-file refusal of semantic review.
//!
//! A repository whose source must never reach a third-party model names a file,
//! and the site policy names that filename. The whole feature is one property:
//! the decision is taken before anything is rendered, before any credential is
//! resolved and before the response cache is consulted, and it arrives as an
//! unanalyzed result rather than a pass.
//!
//! Every test here asserts one of those two halves - the refusal happened, or
//! nothing was contacted. A test asserting only the exit code would pass for a
//! refusal implemented after the request, which is the bypass this file exists to
//! catch. The cache, push-gate and review-budget modes are pinned next door in
//! `marker_refusal_modes`.

use std::path::{Path, PathBuf};

use wiremock::MockServer;

use super::support::{
    check_args, run_drep_with_path_prefix, run_drep_with_site, write_site_policy,
};
use crate::cli::MachineFiles;
use crate::cli::check;
use crate::llm::cache::Cache;
use crate::test_support::{
    git_init, request_count, server_returning, write_drep_toml, write_executable,
};

/// The marker filename these tests configure.
///
/// Arbitrary: presence is the whole signal, and drep attaches no meaning to the
/// name beyond what the site policy says.
pub(super) const MARKER: &str = ".drep-no-llm";

/// A git repository with one provider pointed at a live mock server and a policy
/// naming `markers`.
///
/// The server answers cleanly rather than being dead, on purpose. A dead endpoint
/// exits 2 by itself, so a refusal that never happened would still satisfy the
/// exit assertion; a server that answers makes exit 2 mean the refusal alone.
pub(super) async fn repo(markers: &[&str]) -> (tempfile::TempDir, MockServer, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    git_init(dir.path());
    let server = server_returning(&[r#"{"issues": []}"#]).await;
    write_drep_toml(dir.path(), &format!("{}/v1", server.uri()));
    let site = write_site_policy(dir.path(), markers);
    std::fs::write(dir.path().join("lib.py"), "x = 1\n").expect("lib.py");
    (dir, server, site)
}

/// Run a paths-mode check in process, against a named policy file.
///
/// In process because `run_against` renders to the real stdout: a test that needs
/// to read the output uses [`run_drep_with_site`] instead, and one that needs only
/// the verdict and the request count is cheaper this way.
pub(super) async fn check_in(dir: &Path, root: &Path, site: &Path) -> anyhow::Result<check::Exit> {
    check_paths(dir, root, site, vec![root.join("lib.py")]).await
}

/// [`check_in`] over an explicit path list.
///
/// Separate because the path list is the whole point of the scope tests next
/// door: the files a run reviews are not always the ones under `root`.
pub(super) async fn check_paths(
    dir: &Path,
    root: &Path,
    site: &Path,
    paths: Vec<PathBuf>,
) -> anyhow::Result<check::Exit> {
    check::run_against(
        &check_args(paths, None),
        root,
        Cache::new(dir.join("test-cache"), 30, 8 * 1024 * 1024),
        &MachineFiles {
            auth: &dir.join("auth.toml"),
            policy: site,
        },
    )
    .await
}

#[tokio::test]
async fn a_marker_at_the_repository_root_refuses_semantic_review() {
    let (dir, server, site) = repo(&[MARKER]).await;
    std::fs::write(dir.path().join(MARKER), "").expect("marker");

    let output = run_drep_with_site(dir.path(), &site, &["check", "lib.py"]);
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert_eq!(
        output.status.code(),
        Some(2),
        "a review that did not happen is never a pass; stdout: {stdout}"
    );
    assert!(
        stdout.contains(MARKER),
        "a developer hitting this needs to see which file made it deliberate \
         policy rather than a broken install; stdout: {stdout}"
    );
    assert!(
        stdout.contains(&site.display().to_string()),
        "and which policy asked for it, so they know who to talk to; stdout: {stdout}"
    );
    assert_eq!(
        request_count(&server).await,
        0,
        "the point of the feature is that the source never leaves the machine; \
         stdout: {stdout}"
    );
}

/// The discriminating half of every test above: a policy naming a marker that is
/// absent changes nothing.
///
/// Without it, "refuse whenever `refuse_markers` is set" passes the whole file.
#[tokio::test]
async fn an_unmarked_repository_reviews_normally_when_markers_are_configured() {
    let (dir, server, site) = repo(&[MARKER]).await;

    let exit = check_in(dir.path(), dir.path(), &site)
        .await
        .expect("a configured policy whose marker is absent permits review");

    assert_eq!(exit, check::Exit::Clean);
    assert_eq!(request_count(&server).await, 1);
}

/// The repository root is the unit of policy, not the current directory.
///
/// `cd src && drep check` is the same repository's source. A presence test against
/// the working directory would walk straight past the marker.
#[tokio::test]
async fn a_check_from_a_subdirectory_of_a_marked_repository_is_refused() {
    let (dir, server, site) = repo(&[MARKER]).await;
    std::fs::write(dir.path().join(MARKER), "").expect("marker");
    let sub = dir.path().join("service");
    std::fs::create_dir(&sub).expect("subdirectory");
    write_drep_toml(&sub, &format!("{}/v1", server.uri()));
    std::fs::write(sub.join("lib.py"), "x = 1\n").expect("lib.py");

    let exit = check_in(dir.path(), &sub, &site)
        .await
        .expect("a refusal is a verdict, not a crash");

    assert_eq!(exit, check::Exit::Unanalyzed);
    assert_eq!(
        request_count(&server).await,
        0,
        "a subdirectory of a marked repository is still marked"
    );
}

/// Only the root is consulted. Neither a walk downward nor a search upward.
///
/// A recursive search would let any vendored dependency carrying the filename
/// switch review off for the whole repository.
#[tokio::test]
async fn only_the_repository_root_is_consulted() {
    let (dir, server, site) = repo(&[MARKER]).await;
    let sub = dir.path().join("service");
    std::fs::create_dir(&sub).expect("subdirectory");
    std::fs::write(sub.join(MARKER), "").expect("marker");

    let exit = check_in(dir.path(), dir.path(), &site)
        .await
        .expect("a marker below the root is not this repository's marker");

    assert_eq!(exit, check::Exit::Clean);
    assert_eq!(request_count(&server).await, 1);
}

/// Presence is the whole signal, in all four shapes a name at the root can take.
///
/// An empty file and one whose text says `allow` refuse identically, because
/// nothing opens the file: a marker grammar nobody documented is a grammar
/// somebody would eventually get wrong in the permissive direction. A directory
/// and a symlink whose target is gone refuse too - both are names someone
/// deliberately placed, and an `is_file()` or `metadata` test would let either
/// silently disable the policy.
#[tokio::test]
async fn marker_contents_are_never_consulted() {
    for shape in ["empty", "allow", "directory", "dangling symlink"] {
        let (dir, server, site) = repo(&[MARKER]).await;
        let marker = dir.path().join(MARKER);
        match shape {
            "empty" => std::fs::write(&marker, "").expect("marker"),
            "allow" => std::fs::write(&marker, "allow\n").expect("marker"),
            "directory" => std::fs::create_dir(&marker).expect("marker"),
            _ => {
                #[cfg(unix)]
                std::os::unix::fs::symlink(dir.path().join("gone"), &marker).expect("marker");
                #[cfg(not(unix))]
                continue;
            }
        }

        let exit = check_in(dir.path(), dir.path(), &site)
            .await
            .unwrap_or_else(|err| panic!("{shape}: {err:#}"));

        assert_eq!(exit, check::Exit::Unanalyzed, "{shape}");
        assert_eq!(request_count(&server).await, 0, "{shape}");
    }
}

/// The first marker in the list is not the only one that counts.
#[tokio::test]
async fn any_of_several_configured_markers_refuses() {
    let (dir, server, site) = repo(&[".no-cloud-review", MARKER]).await;
    std::fs::write(dir.path().join(MARKER), "").expect("marker");

    let exit = check_in(dir.path(), dir.path(), &site)
        .await
        .expect("a refusal is a verdict, not a crash");

    assert_eq!(exit, check::Exit::Unanalyzed);
    assert_eq!(request_count(&server).await, 0);
}

/// The deterministic half still runs, and a failure still outranks its findings.
///
/// The tools are local and contact nothing, so refusing the model is no reason to
/// stop linting - and exit 1 here would tell a developer "fix these findings" for
/// a run that also never reviewed the code.
#[tokio::test]
async fn deterministic_findings_still_run_under_a_refusal() {
    let (dir, server, site) = repo(&[MARKER]).await;
    std::fs::write(dir.path().join(MARKER), "").expect("marker");
    std::fs::write(dir.path().join("pyproject.toml"), "").expect("pyproject");
    let ruff = dir.path().join("venv/bin/ruff");
    std::fs::create_dir_all(ruff.parent().expect("bin dir")).expect("bin dir");
    write_executable(
        &ruff,
        "#!/bin/sh\nprintf '%s' '[{\"filename\":\"lib.py\",\
         \"location\":{\"row\":1,\"column\":1},\"code\":\"F401\",\
         \"message\":\"unused import\"}]'\n",
    );

    let output = run_drep_with_site(dir.path(), &site, &["check", "lib.py"]);
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        stdout.contains("unused import"),
        "the half of drep that works without a model still has to run; \
         stdout: {stdout}"
    );
    assert_eq!(
        output.status.code(),
        Some(2),
        "a failure outranks a finding, so this is 2 and not 1; stdout: {stdout}"
    );
    assert_eq!(request_count(&server).await, 0, "stdout: {stdout}");
}

/// A refused repository never mints a credential.
///
/// `api_key_command` exists for gateways handing out short-lived tokens, so
/// running it is a request to a third party on behalf of a repository whose source
/// is not allowed to reach one. It also cannot be fatal here: a broken helper must
/// not be what a refused run reports.
#[tokio::test]
async fn a_refused_repository_never_runs_its_key_command() {
    let (dir, server, site) = repo(&[MARKER]).await;
    std::fs::write(dir.path().join(MARKER), "").expect("marker");
    let sentinel = dir.path().join("command-ran");
    let stub = dir.path().join("print-token");
    write_executable(
        &stub,
        format!(
            "#!/bin/sh\nprintf '%s' ran > {}\nexit 7\n",
            sentinel.to_string_lossy()
        ),
    );
    std::fs::write(
        dir.path().join("drep.toml"),
        format!(
            "[[llm]]\nendpoint = \"{}/v1\"\nmodel = \"m\"\napi_key_command = [{:?}]\n",
            server.uri(),
            stub.to_string_lossy()
        ),
    )
    .expect("drep.toml");

    let exit = check_in(dir.path(), dir.path(), &site)
        .await
        .expect("a failing helper must not be what a refused run reports");

    assert_eq!(exit, check::Exit::Unanalyzed);
    assert!(
        !sentinel.exists(),
        "the refusal has to precede credential resolution, not follow it"
    );
}

/// A `codex` entry is refused without the Codex CLI ever being asked about.
///
/// Building the chain probes ChatGPT login state by spawning the CLI, so a chain
/// built ahead of the refusal starts the model machinery for a repository whose
/// source must never reach a model. On a machine without the CLI installed the
/// probe also fails, and a refused run reporting *that* would be reporting the
/// wrong thing entirely.
///
/// A fake `codex` at the front of `PATH` is what makes the property checkable
/// either way: the exit code alone is 2 on a machine with the real CLI installed
/// whether the refusal came first or the codex run simply failed, and 2 on a
/// machine without it because the probe errored. The sentinel says which.
#[tokio::test]
async fn a_codex_backend_is_refused_without_building_the_chain() {
    let (dir, _server, site) = repo(&[MARKER]).await;
    std::fs::write(dir.path().join(MARKER), "").expect("marker");
    std::fs::write(
        dir.path().join("drep.toml"),
        "[[llm]]\nbackend = \"codex\"\nmodel = \"m\"\n",
    )
    .expect("drep.toml");
    let fake_bin = dir.path().join("fake-bin");
    std::fs::create_dir(&fake_bin).expect("fixture PATH entry");
    let sentinel = dir.path().join("codex-ran");
    write_executable(
        &fake_bin.join("codex"),
        format!(
            "#!/bin/sh\nprintf '%s' ran > {}\nexit 0\n",
            sentinel.to_string_lossy()
        ),
    );

    let output = run_drep_with_path_prefix(dir.path(), &site, &fake_bin, &["check", "lib.py"]);
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert_eq!(output.status.code(), Some(2), "stdout: {stdout}");
    assert!(
        !sentinel.exists(),
        "the refusal has to answer before the backend is built, not after; \
         stdout: {stdout}"
    );
}
