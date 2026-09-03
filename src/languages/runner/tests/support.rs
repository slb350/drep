//! Shared fixtures: tool specs and filesystem helpers.
//!
//! Wired in via `#[cfg(test)] mod tests;` in the parent module. These files were
//! orphaned once - present on disk but reachable by no `mod` declaration, so
//! cargo never compiled them and appending invalid Rust did not fail the build.
//! If you add a file here, declare it in this directory's `mod.rs`.

use crate::languages::spec::ToolSpec;

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
        ..ToolSpec::default()
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
        ..ToolSpec::default()
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
        ..ToolSpec::default()
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
        ..ToolSpec::default()
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
        ..ToolSpec::default()
    }
}

/// Re-exported so this suite's `use super::support::*` importers reach the
/// crate-wide helper rather than a local copy.
pub(crate) use crate::test_support::write_executable;

/// checkstyle emits SARIF 2.1.0 on stdout. `-c` is appended by `config_flag`
/// rather than written here, since which ruleset a project uses is exactly
/// what `config_files` discovers.
pub(crate) fn checkstyle_like_spec() -> ToolSpec {
    ToolSpec {
        name: "checkstyle",
        command: &["checkstyle", "-f", "sarif"],
        local_paths: &[],
        config_files: &["checkstyle.xml"],
        config_flag: Some("-c"),
        output_format: "sarif",
        diagnostics_stream: "stdout",
        ..ToolSpec::default()
    }
}

pub(crate) fn ktlint_like_spec() -> ToolSpec {
    ToolSpec {
        name: "ktlint",
        command: &["ktlint", "--log-level=none", "--reporter=json"],
        local_paths: &[],
        config_files: &[".editorconfig"],
        output_format: "ktlint",
        diagnostics_stream: "stdout",
        ..ToolSpec::default()
    }
}

pub(crate) fn shellcheck_like_spec() -> ToolSpec {
    ToolSpec {
        name: "shellcheck",
        command: &["shellcheck", "-f", "json"],
        local_paths: &[],
        config_files: &[".shellcheckrc"],
        output_format: "shellcheck",
        diagnostics_stream: "stdout",
        ..ToolSpec::default()
    }
}

pub(crate) fn rubocop_like_spec() -> ToolSpec {
    ToolSpec {
        name: "rubocop",
        command: &["rubocop", "--format", "json", "--force-exclusion"],
        local_paths: &["bin/rubocop"],
        config_files: &[".rubocop.yml"],
        output_format: "rubocop",
        diagnostics_stream: "stdout",
        ..ToolSpec::default()
    }
}

pub(crate) fn phpcs_like_spec() -> ToolSpec {
    ToolSpec {
        name: "phpcs",
        command: &["phpcs", "--report=json"],
        local_paths: &["vendor/bin/phpcs"],
        config_files: &["phpcs.xml"],
        output_format: "phpcs",
        diagnostics_stream: "stdout",
        ..ToolSpec::default()
    }
}

pub(crate) fn credo_like_spec() -> ToolSpec {
    ToolSpec {
        name: "credo",
        command: &["mix", "credo", "--format", "json"],
        local_paths: &[],
        config_files: &[".credo.exs"],
        output_format: "credo",
        diagnostics_stream: "stdout",
        ..ToolSpec::default()
    }
}

pub(crate) fn sqlfluff_like_spec() -> ToolSpec {
    ToolSpec {
        name: "sqlfluff",
        command: &["sqlfluff", "lint", "--format", "json"],
        local_paths: &["venv/bin/sqlfluff"],
        config_files: &[".sqlfluff"],
        output_format: "sqlfluff",
        diagnostics_stream: "stdout",
        ..ToolSpec::default()
    }
}

pub(crate) fn dotnet_format_like_spec() -> ToolSpec {
    ToolSpec {
        name: "dotnet format",
        command: &["dotnet", "format", "--verify-no-changes", "--no-restore"],
        local_paths: &[],
        config_files: &[".editorconfig"],
        output_format: "msbuild",
        diagnostics_stream: "stdout",
        ..ToolSpec::default()
    }
}
