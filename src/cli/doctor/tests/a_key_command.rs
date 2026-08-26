//! `api_key_command` in the LLM section: what doctor says about a credential
//! helper, and what it must never say.
//!
//! doctor's contract is "what will actually run here", so the helper is really
//! invoked. What that means for output is the whole point of these three tests: a
//! working helper is reported as working, a broken one as broken, and neither
//! line carries a byte of what the helper printed.

use std::path::Path;

use crate::cli::doctor::{DoctorArgs, run_at};
use crate::test_support::write_executable;

fn args(dir: &Path) -> DoctorArgs {
    DoctorArgs {
        path: dir.to_path_buf(),
        config: None,
    }
}

/// Run `doctor` against `dir` with an auth store scoped to it.
async fn report_for(dir: &Path) -> String {
    let mut out = Vec::new();
    let exit = run_at(&mut out, &args(dir), &dir.join("auth.toml"))
        .await
        .expect("run_at");
    assert_eq!(
        exit,
        crate::Exit::Clean,
        "a credential diagnosis is never a gate failure"
    );
    String::from_utf8(out).expect("utf8")
}

/// Write a `drep.toml` whose only provider mints its key by running `argv`.
fn write_config_running(dir: &Path, argv: &str) {
    std::fs::write(
        dir.join("drep.toml"),
        format!("[[llm]]\nendpoint = \"http://e/v1\"\nmodel = \"m\"\napi_key_command = [{argv}]\n"),
    )
    .expect("drep.toml");
}

#[tokio::test]
async fn a_working_command_is_reported_as_working_without_its_output() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stub = dir.path().join("print-token");
    write_executable(&stub, "#!/bin/sh\nprintf '%s' 'sk-live-sekrit'\n");
    write_config_running(dir.path(), &format!("{:?}", stub.to_string_lossy()));

    let report = report_for(dir.path()).await;

    assert!(
        report.contains("from api_key_command"),
        "the source must be named, so `doctor` and `check` agree about it: {report}"
    );
    assert!(
        report.contains("the command ran and printed a credential"),
        "a helper that works is worth saying so: {report}"
    );
    assert!(
        !report.contains("sk-live-sekrit"),
        "doctor output is what people paste into bug reports and CI logs: {report}"
    );
}

#[tokio::test]
async fn a_failing_command_is_reported_with_its_status_and_never_its_output() {
    // The reason the probe exists. A helper that stopped authenticating is the
    // failure `api_key_command` was configured to surface, and a doctor that
    // reported it as fine would be describing a run that exits 2.
    let dir = tempfile::tempdir().expect("tempdir");
    let stub = dir.path().join("print-token");
    write_executable(
        &stub,
        "#!/bin/sh\nprintf '%s' 'sk-live-sekrit'\nprintf '%s' 'sk-live-sekrit' >&2\nexit 9\n",
    );
    write_config_running(dir.path(), &format!("{:?}", stub.to_string_lossy()));

    let report = report_for(dir.path()).await;

    assert!(report.contains("print-token"), "got {report}");
    assert!(
        report.contains('9'),
        "the exit status is the actionable half: {report}"
    );
    assert!(
        !report.contains("sk-live-sekrit"),
        "a misconfigured helper prints the token to both streams; the diagnostic \
         is thin so neither reaches the report: {report}"
    );
}

#[tokio::test]
async fn a_command_naming_an_unset_variable_is_reported_as_not_attempted() {
    // The probe runs the *expanded* argv, so when the config does not load there
    // is no argv to run. Executing the raw one would report on a command
    // containing a literal `${VAR}`, which is not the command `check` runs.
    let dir = tempfile::tempdir().expect("tempdir");
    write_config_running(
        dir.path(),
        "\"print-token\", \"${DREP_DOCTOR_KEY_CMD_UNSET}\"",
    );

    let report = report_for(dir.path()).await;

    assert!(
        report.contains("not attempted"),
        "an argv drep cannot expand is not a command it can probe: {report}"
    );
    assert!(
        report.contains("DREP_DOCTOR_KEY_CMD_UNSET is NOT set"),
        "and the reader is told which variable to export: {report}"
    );
}
