//! Tool resolution and eligibility: resolve_tool, is_configured, tool_status.
//!
//! Wired in via `#[cfg(test)] mod tests;` in the parent module. These files were
//! orphaned once - present on disk but reachable by no `mod` declaration, so
//! cargo never compiled them and appending invalid Rust did not fail the build.
//! If you add a file here, declare it in this directory's `mod.rs`.

use tempfile::TempDir;

use super::support::*;
use crate::languages::runner::*;

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

#[test]
fn non_executable_repo_local_path_falls_through_to_path() {
    let _guard = PATH_LOCK.lock().unwrap();
    let dir = TempDir::new().unwrap();
    // exists, but not executable - must be skipped, not picked.
    std::fs::write(dir.path().join("mytool"), "not executable").unwrap();

    let spec = ToolSpec {
        local_paths: &["mytool"],
        command: &["mytool"],
        ..ToolSpec::default()
    };

    // Falls through to PATH; `/bin/echo` is on every unix.
    assert_eq!(resolve_tool(&spec, dir.path()), None);
    let resolved = resolve_tool(
        &ToolSpec {
            local_paths: &[],
            command: &["echo"],
            ..ToolSpec::default()
        },
        dir.path(),
    );
    assert!(resolved.is_some(), "/bin/echo should be on PATH");
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
