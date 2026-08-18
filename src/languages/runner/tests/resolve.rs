//! Tool resolution and eligibility: resolve_tool, is_configured, tool_status.
//!
//! Wired in via `#[cfg(test)] mod tests;` in the parent module. These files were
//! orphaned once - present on disk but reachable by no `mod` declaration, so
//! cargo never compiled them and appending invalid Rust did not fail the build.
//! If you add a file here, declare it in this directory's `mod.rs`.

use tempfile::TempDir;

use super::support::*;
use crate::languages::runner::*;
use crate::languages::spec::ToolSpec;

// ---- resolve_tool ----

#[test]
fn repo_local_executable_is_preferred_over_path() {
    let dir = TempDir::new().unwrap();
    let bin = dir.path().join("mytool");
    std::fs::write(&bin, "#!/bin/sh\n").unwrap();
    make_executable(&bin);

    let spec = ToolSpec {
        local_paths: &["mytool"],
        command: &["mytool"],
        ..ToolSpec::default()
    };

    assert_eq!(resolve_tool(&spec, dir.path()), Some(bin));
}

/// A non-executable repo-local hit is skipped, and resolution continues to
/// PATH rather than stopping.
///
/// The fallthrough is the half that needs an executable on PATH to observe.
/// Asserting only `None` proved nothing: a `resolve_tool` that returned `None`
/// the moment it saw a non-executable local path would pass identically, and
/// the assertion also depended on the host not happening to have a `mytool`
/// installed.
#[test]
fn non_executable_repo_local_path_falls_through_to_path() {
    let dir = TempDir::new().unwrap();

    // Repo-local hit that exists but is not executable: must be skipped.
    std::fs::write(dir.path().join("mytool"), "not executable").unwrap();

    // The same name, executable, on the PATH we hand in. Resolution must
    // reach it rather than stopping at the non-executable local hit.
    let path_dir = TempDir::new().unwrap();
    let on_path = path_dir.path().join("mytool");
    std::fs::write(&on_path, "#!/bin/sh\nexit 0\n").unwrap();
    make_executable(&on_path);

    let spec = ToolSpec {
        local_paths: &["mytool"],
        command: &["mytool"],
        ..ToolSpec::default()
    };

    let path = std::env::join_paths([path_dir.path()]).expect("joins");
    assert_eq!(
        resolve_tool_in(&spec, dir.path(), Some(path.as_os_str())),
        Some(on_path),
        "the non-executable local hit must be skipped and PATH consulted"
    );
}

#[test]
fn resolve_tool_returns_none_when_neither_local_nor_path_has_it() {
    let dir = TempDir::new().unwrap();
    let spec = ToolSpec {
        local_paths: &["definitely-not-installed-x9z"],
        command: &["definitely-not-installed-x9z"],
        ..ToolSpec::default()
    };
    assert_eq!(resolve_tool(&spec, dir.path()), None);
}

// ---- is_configured / tool_status ----

#[test]
fn is_configured_true_when_any_config_file_exists() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("pyproject.toml"), "").unwrap();
    assert!(is_configured(&ruff_like_spec(), dir.path()));
}

#[test]
fn tool_status_skipped_when_not_configured_and_detail_lists_config() {
    let dir = TempDir::new().unwrap();
    let outcome = tool_status(&ruff_like_spec(), dir.path());
    assert_eq!(outcome.status, ToolStatus::Skipped);
    // Detail must name a config file, so the user can act on it.
    assert!(
        outcome.detail.contains("pyproject.toml"),
        "detail was {:?}",
        outcome.detail
    );
}

#[test]
fn tool_status_unavailable_when_configured_but_binary_missing() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("pyproject.toml"), "").unwrap();
    let spec = ToolSpec {
        name: "ruff",
        local_paths: &["no/such/ruff"],
        command: &["definitely-not-installed-ruff-abc"],
        config_files: &["pyproject.toml"],
        output_format: "json",
        diagnostics_stream: "stdout",
    };
    let outcome = tool_status(&spec, dir.path());
    assert_eq!(outcome.status, ToolStatus::Unavailable);
}

#[test]
fn passed_is_false_only_for_unavailable() {
    // The invariant that matters most: an unavailable tool must never
    // read as a pass. Skipped and Ok both genuinely mean the tool got
    // to look at the code (or deliberately chose not to).
    let mut outcome = ToolOutcome {
        tool: "ruff",
        status: ToolStatus::Ok,
        findings: vec![],
        detail: String::new(),
    };
    assert!(outcome.passed());
    outcome.status = ToolStatus::Skipped;
    assert!(outcome.passed());
    outcome.status = ToolStatus::Unavailable;
    assert!(!outcome.passed());
}

/// A file whose name begins with `-` is passed as a path, not an option.
///
/// A repository can legitimately contain `--fix`, and every checker drep runs
/// would read that as a flag. The guard is a `./` prefix rather than a `--`
/// separator, because `--` is not universally accepted across
/// ruff/eslint/tsc/gofmt/go vet/clippy while `./` is unambiguous to any
/// argument parser.
///
/// Resolves the fake tool through `local_paths`, so it touches no global
/// state at all.
#[tokio::test]
async fn a_filename_that_looks_like_a_flag_is_passed_as_a_path() {
    let dir = tempfile::tempdir().expect("tempdir");
    let bin = dir.path().join("bin");
    std::fs::create_dir_all(&bin).expect("mkdir bin");
    let tool = bin.join("argvdump");
    std::fs::write(
        &tool,
        format!(
            "#!/bin/sh\nfor a in \"$@\"; do echo \"$a\"; done > {}/argv.txt\nexit 0\n",
            dir.path().display()
        ),
    )
    .expect("write tool");
    make_executable(&tool);
    std::fs::write(dir.path().join("pyproject.toml"), "").expect("config");

    let spec = ToolSpec {
        name: "argvdump",
        command: &["argvdump"],
        local_paths: &["bin/argvdump"],
        config_files: &["pyproject.toml"],
        output_format: "lines",
        diagnostics_stream: "stdout",
    };

    let _ = run_tool(&spec, dir.path(), &["--fix".to_owned()]).await;

    let argv = std::fs::read_to_string(dir.path().join("argv.txt")).unwrap_or_default();
    assert!(
        argv.lines().any(|a| a == "./--fix"),
        "a dash-leading filename must reach the tool as `./--fix`, got: {argv:?}"
    );
    assert!(
        !argv.lines().any(|a| a == "--fix"),
        "it must not reach the tool as a bare option: {argv:?}"
    );
}
