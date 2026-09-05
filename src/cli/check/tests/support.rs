//! Shared fixtures for the `check` suite.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use assert_cmd::Command;

use crate::Exit;
use crate::analysis::findings::{Finding, Severity};
use crate::analysis::result::FailureReason;
use crate::cli::OutputFormat;
use crate::cli::check::{CheckArgs, CheckOutcome, ProviderUse, render};

/// Build the common paths-mode check arguments used across orchestration tests.
pub(super) fn check_args(paths: Vec<PathBuf>, fail_on: Option<Severity>) -> CheckArgs {
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
        max_review_rounds: None,
        unlimited_reviews: false,
    }
}

/// Run the drep test binary with an isolated cache and bounded network access.
///
/// The policy override names an absent file in `dir`. An installed machine policy
/// still takes precedence; subprocesses cannot displace it. In-process policy
/// tests instead inject the policy path directly.
pub(super) fn run_drep(dir: &Path, args: &[&str]) -> std::process::Output {
    run_drep_with_site(dir, &dir.join("absent-site.toml"), args)
}

/// [`run_drep`] against a named site policy file.
pub(super) fn run_drep_with_site(
    dir: &Path,
    site_path: &Path,
    args: &[&str],
) -> std::process::Output {
    spawn_drep(dir, site_path, None, args)
}

/// [`run_drep_with_site`] with `first_on_path` ahead of the inherited `PATH`.
///
/// Prepended, not replaced: drep spawns `git` to resolve a repository root, so a
/// `PATH` holding only the fixture directory would fail for a reason that has
/// nothing to do with what the test is about.
pub(super) fn run_drep_with_path_prefix(
    dir: &Path,
    site_path: &Path,
    first_on_path: &Path,
    args: &[&str],
) -> std::process::Output {
    spawn_drep(dir, site_path, Some(first_on_path), args)
}

fn spawn_drep(
    dir: &Path,
    site_path: &Path,
    first_on_path: Option<&Path>,
    args: &[&str],
) -> std::process::Output {
    let bin = assert_cmd::cargo::cargo_bin("drep");
    let mut command = Command::new(bin);
    command
        .args(args)
        .current_dir(dir)
        .env("HOME", dir)
        .env("XDG_CACHE_HOME", dir)
        .env(crate::config::site::PATH_VAR, site_path)
        .env_remove("HTTP_PROXY")
        .env_remove("HTTPS_PROXY")
        .env_remove("ALL_PROXY")
        .env_remove("http_proxy")
        .env_remove("https_proxy")
        .env_remove("all_proxy")
        .timeout(Duration::from_secs(15));
    if let Some(first) = first_on_path {
        let inherited = std::env::var_os("PATH").unwrap_or_default();
        let mut entries = vec![first.to_path_buf()];
        entries.extend(std::env::split_paths(&inherited));
        command.env(
            "PATH",
            std::env::join_paths(entries).expect("a joinable PATH"),
        );
    }
    command.output().expect("drep spawns and finishes")
}

/// A `CheckOutcome` with everything empty and the gate clean.
pub(super) fn outcome() -> CheckOutcome {
    CheckOutcome {
        tool_findings: Vec::new(),
        llm_findings: Vec::new(),
        failures: BTreeMap::new(),
        provider_uses: Vec::new(),
        retry_push: false,
        review_activity: None,
        exit: Exit::Clean,
    }
}

/// An outcome carrying `failures`, with the exit code they imply.
///
/// The exit is derived rather than passed: a failure means exit 2, and a test
/// that could state otherwise would be pinning a combination `run` cannot
/// produce.
pub(super) fn outcome_failing(failures: Vec<(&str, FailureReason)>) -> CheckOutcome {
    let failures: BTreeMap<PathBuf, FailureReason> = failures
        .into_iter()
        .map(|(path, reason)| (PathBuf::from(path), reason))
        .collect();
    CheckOutcome {
        exit: if failures.is_empty() {
            Exit::Clean
        } else {
            Exit::Unanalyzed
        },
        failures,
        ..outcome()
    }
}

/// An outcome carrying tool findings and the exit they imply.
pub(super) fn outcome_with_tool_findings(tool_findings: Vec<Finding>) -> CheckOutcome {
    CheckOutcome {
        exit: if tool_findings.is_empty() {
            Exit::Clean
        } else {
            Exit::FoundIssues
        },
        tool_findings,
        ..outcome()
    }
}

/// Render `outcome` and return what it wrote.
pub(super) fn rendered(outcome: &CheckOutcome, format: OutputFormat) -> String {
    let mut buf: Vec<u8> = Vec::new();
    render::render_to(&mut buf, outcome, format).expect("render");
    String::from_utf8(buf).expect("utf8")
}

/// Render `outcome` as JSON and parse it.
pub(super) fn rendered_json(outcome: &CheckOutcome) -> serde_json::Value {
    serde_json::from_str(&rendered(outcome, OutputFormat::Json)).expect("valid JSON")
}

/// One entry for the `provider_uses` field.
pub(super) fn provider_use(index: usize, model: &str, location: &str, files: usize) -> ProviderUse {
    ProviderUse {
        index,
        model: model.to_owned(),
        location: location.to_owned(),
        files,
    }
}
