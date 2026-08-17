//! Shared fixtures: tool specs and filesystem helpers.
//!
//! Wired in via `#[cfg(test)] mod tests;` in the parent module. These files were
//! orphaned once - present on disk but reachable by no `mod` declaration, so
//! cargo never compiled them and appending invalid Rust did not fail the build.
//! If you add a file here, declare it in this directory's `mod.rs`.

use std::path::Path;
use std::sync::Mutex;

use crate::languages::spec::ToolSpec;

// Serialize the PATH-mutating tests. They prepend a temp directory to
// PATH to make a fake binary resolvable, which would race if two ran in
// parallel.
pub(crate) static PATH_LOCK: Mutex<()> = Mutex::new(());

/// A minimal ruff-shaped spec for parser tests, avoiding the real
/// `definitions::RUFF` so the parser logic can be exercised without
/// needing ruff installed.
pub(crate) fn ruff_like_spec() -> ToolSpec {
    ToolSpec {
        name: "ruff",
        command: &["ruff", "check", "--output-format", "json"],
        local_paths: &["venv/bin/ruff"],
        config_files: &["pyproject.toml"],
        output_format: "json",
        diagnostics_stream: "stdout",
    }
}

pub(crate) fn gofmt_like_spec() -> ToolSpec {
    ToolSpec {
        name: "gofmt",
        command: &["gofmt", "-l"],
        local_paths: &[],
        config_files: &["go.mod"],
        output_format: "lines",
        diagnostics_stream: "stdout",
    }
}

pub(crate) fn go_vet_like_spec() -> ToolSpec {
    ToolSpec {
        name: "go vet",
        command: &["go", "vet"],
        local_paths: &[],
        config_files: &["go.mod"],
        output_format: "position",
        diagnostics_stream: "stderr",
    }
}

pub(crate) fn tsc_like_spec() -> ToolSpec {
    ToolSpec {
        name: "tsc",
        command: &["tsc", "--noEmit", "--pretty", "false"],
        local_paths: &["node_modules/.bin/tsc"],
        config_files: &["tsconfig.json"],
        output_format: "tsc",
        diagnostics_stream: "stdout",
    }
}

pub(crate) fn clippy_like_spec() -> ToolSpec {
    ToolSpec {
        name: "clippy",
        command: &["cargo", "clippy", "--message-format", "json"],
        local_paths: &[],
        config_files: &["Cargo.toml"],
        output_format: "cargo",
        diagnostics_stream: "stdout",
    }
}

// ---- helpers ----

/// Mark a file executable on unix; no-op elsewhere.
pub(crate) fn make_executable(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(path).unwrap().permissions();
        perms.set_mode(perms.mode() | 0o111);
        std::fs::set_permissions(path, perms).unwrap();
    }
    #[cfg(not(unix))]
    let _ = path;
}
