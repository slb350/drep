//! Shared fixtures for the `check` suite.
//!
//! The `CheckOutcome` literal was written out in three files - the text-output
//! test, the `unanalyzed` JSON test, and the failover report test. Adding
//! `provider_uses` to the struct meant editing all three, which is the tax
//! `test_support::write_drep_toml`'s doc records as a past bug: a missed copy
//! surfaces not as a failed assertion but as a compile error in a test that
//! looks unrelated.

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
pub(super) fn run_drep(dir: &Path, args: &[&str]) -> std::process::Output {
    let bin = assert_cmd::cargo::cargo_bin("drep");
    let mut command = Command::new(bin);
    command
        .args(args)
        .current_dir(dir)
        .env("HOME", dir)
        .env("XDG_CACHE_HOME", dir)
        .env_remove("HTTP_PROXY")
        .env_remove("HTTPS_PROXY")
        .env_remove("ALL_PROXY")
        .env_remove("http_proxy")
        .env_remove("https_proxy")
        .env_remove("all_proxy")
        .timeout(Duration::from_secs(15));
    command.output().expect("drep spawns and finishes")
}

/// A `CheckOutcome` with everything empty and the gate clean.
///
/// Callers fill in only the field their test is about, so a new field on
/// `CheckOutcome` costs one default here rather than an edit per test.
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
