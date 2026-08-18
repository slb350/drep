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

/// A tool that exits non-zero having produced no diagnostics is `Unavailable`,
/// not a clean `Ok`.
///
/// The exit code alone is not a verdict - ruff and clippy exit non-zero
/// *because* they found issues - but a non-zero exit with nothing on the
/// diagnostics stream means the tool did not run: bad config, crash, bad
/// invocation. Reporting that as `Ok` with zero findings is the
/// "unavailable is not a pass" failure this module exists to prevent.
#[tokio::test]
async fn a_silent_non_zero_exit_is_unavailable_not_a_clean_pass() {
    let dir = TempDir::new().unwrap();
    let bin = dir.path().join("failtool");
    std::fs::write(&bin, "#!/bin/sh\necho 'fatal: bad config' >&2\nexit 2\n").unwrap();
    make_executable(&bin);
    std::fs::write(dir.path().join("pyproject.toml"), "").unwrap();

    // `lines`, not `json`. An empty stdout is not valid JSON, so a `json`
    // spec reaches `Unavailable` through the *parse-failure* path and the test
    // passes without ever exercising the exit-status rule - which is exactly
    // what happened on the first draft. The `lines` parser accepts empty input
    // as zero findings, so only the new rule can produce `Unavailable` here.
    let spec = ToolSpec {
        name: "failtool",
        command: &["failtool"],
        local_paths: &["failtool"],
        config_files: &["pyproject.toml"],
        output_format: "lines",
        diagnostics_stream: "stdout",
    };

    let outcome = run_tool(&spec, dir.path(), &["a.py".to_owned()]).await;
    assert_eq!(
        outcome.status,
        ToolStatus::Unavailable,
        "a silent non-zero exit must not read as a clean pass, got {outcome:?}"
    );
    assert!(
        outcome.detail.contains("fatal: bad config"),
        "the other stream carries the real error and must reach the detail: {}",
        outcome.detail
    );
}

/// A relative `root` still resolves the repo-local tool.
///
/// `resolve_tool` returns `root.join(relative)`, and the child is spawned with
/// `current_dir(root)` - so a relative root made the child resolve the
/// executable a second time, from inside root: `repo/repo/node_modules/...`.
/// It worked only because the CLI passes "." and the tests pass absolute temp
/// dirs.
#[tokio::test]
async fn a_relative_root_still_finds_the_repo_local_tool() {
    let dir = TempDir::new().unwrap();
    let cwd = std::env::current_dir().expect("cwd");
    let relative = pathdiff_relative(&cwd, dir.path());

    let bin = dir.path().join("mytool");
    std::fs::write(&bin, "#!/bin/sh\nexit 0\n").unwrap();
    make_executable(&bin);
    std::fs::write(dir.path().join("pyproject.toml"), "").unwrap();

    let spec = ToolSpec {
        name: "mytool",
        command: &["mytool"],
        local_paths: &["mytool"],
        config_files: &["pyproject.toml"],
        output_format: "lines",
        diagnostics_stream: "stdout",
    };

    let outcome = run_tool(&spec, &relative, &[]).await;
    assert_ne!(
        outcome.status,
        ToolStatus::Unavailable,
        "a relative root must still spawn the repo-local tool, got {outcome:?}"
    );
}

/// A path for `to` expressed relative to `from`, when `to` is absolute and
/// shares no prefix worth trimming: falls back to the absolute path.
fn pathdiff_relative(from: &std::path::Path, to: &std::path::Path) -> std::path::PathBuf {
    to.strip_prefix(from)
        .map(std::path::Path::to_path_buf)
        .unwrap_or_else(|_| to.to_path_buf())
}
