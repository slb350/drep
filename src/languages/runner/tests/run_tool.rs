//! End-to-end run_tool behaviour against real /bin/sh scripts.
//!
//! Wired in via `#[cfg(test)] mod tests;` in the parent module. These files were
//! orphaned once - present on disk but reachable by no `mod` declaration, so
//! cargo never compiled them and appending invalid Rust did not fail the build.
//! If you add a file here, declare it in this directory's `mod.rs`.

use tempfile::TempDir;

use super::support::*;
use crate::languages::runner::*;

// ---- run_tool ----

#[tokio::test]
async fn run_tool_returns_skipped_when_no_config_file_present() {
    let spec = ToolSpec {
        name: "ruff",
        local_paths: &["nope"],
        command: &["definitely-not-installed-ruff-xyz"],
        config_files: &["pyproject.toml"],
        output_format: "json",
        diagnostics_stream: "stdout",
    };
    let dir = TempDir::new().unwrap();
    let outcome = run_tool(&spec, dir.path(), &[]).await;
    assert_eq!(outcome.status, ToolStatus::Skipped);
}

#[tokio::test]
async fn run_tool_returns_unavailable_when_binary_cannot_be_found() {
    let spec = ToolSpec {
        name: "no-such-tool",
        local_paths: &[],
        command: &["definitely-not-installed-tool-zzz"],
        config_files: &["any"],
        output_format: "json",
        diagnostics_stream: "stdout",
    };
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("any"), "").unwrap();
    let outcome = run_tool(&spec, dir.path(), &[]).await;
    assert_eq!(outcome.status, ToolStatus::Unavailable);
}

#[tokio::test]
async fn run_tool_reads_stderr_when_diagnostics_stream_is_stderr() {
    // Build a tiny shell script in a temp dir; it writes parseable JSON
    // to stderr and nothing to stdout. With `diagnostics_stream = "stderr"`
    // we expect one finding; flipping the stream to stdout drops it
    // (stdout is empty, so it parses as `[]`).
    let dir = TempDir::new().unwrap();
    let bin = dir.path().join("diag");
    std::fs::write(
        &bin,
        "#!/bin/sh\nprintf '%s' '[{\"code\":\"E1\",\"filename\":\"x\",\"location\":{\"row\":1,\"column\":1},\"message\":\"m\"}]' 1>&2\n",
    )
    .unwrap();
    make_executable(&bin);

    let spec = ToolSpec {
        name: "diag",
        command: &["diag"],
        local_paths: &["diag"],
        config_files: &["marker"],
        output_format: "json",
        diagnostics_stream: "stderr",
    };
    std::fs::write(dir.path().join("marker"), "").unwrap();

    let outcome = run_tool(&spec, dir.path(), &[]).await;
    assert_eq!(outcome.status, ToolStatus::Ok);
    assert_eq!(outcome.findings.len(), 1);
    assert_eq!(outcome.findings[0].kind, "E1");

    // Same tool with `diagnostics_stream = "stdout"` produces nothing
    // from stderr - the round-trip proves the stream is being honoured.
    let spec_stdout = ToolSpec {
        diagnostics_stream: "stdout",
        ..spec
    };
    let outcome = run_tool(&spec_stdout, dir.path(), &[]).await;
    assert_eq!(outcome.status, ToolStatus::Ok);
    assert!(
        outcome.findings.is_empty(),
        "stdout-streamed run should not pick up stderr diagnostics"
    );
}

#[tokio::test]
async fn run_tool_reports_unavailable_for_unparseable_output_not_empty_ok() {
    let dir = TempDir::new().unwrap();
    let bin = dir.path().join("noisy");
    std::fs::write(&bin, "#!/bin/sh\necho 'this is not json'\n").unwrap();
    make_executable(&bin);

    let spec = ToolSpec {
        name: "noisy",
        command: &["noisy"],
        local_paths: &["noisy"],
        config_files: &["marker"],
        output_format: "json",
        diagnostics_stream: "stdout",
    };
    std::fs::write(dir.path().join("marker"), "").unwrap();

    let outcome = run_tool(&spec, dir.path(), &[]).await;
    assert_eq!(outcome.status, ToolStatus::Unavailable);
    assert!(
        outcome.findings.is_empty(),
        "unparseable must not be reported as zero findings"
    );
}
