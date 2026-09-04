//! Shared fixtures: tool specs and filesystem helpers.

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
        output_format: OutputFormat::Json,
        diagnostics_stream: DiagnosticsStream::Stdout,
        ..ToolSpec::default()
    }
}

pub(crate) fn gofmt_like_spec() -> ToolSpec {
    ToolSpec {
        name: "gofmt",
        command: &["gofmt", "-l"],
        local_paths: &[],
        config_files: &["go.mod"],
        output_format: OutputFormat::Lines,
        diagnostics_stream: DiagnosticsStream::Stdout,
        ..ToolSpec::default()
    }
}

pub(crate) fn go_vet_like_spec() -> ToolSpec {
    ToolSpec {
        name: "go vet",
        command: &["go", "vet"],
        local_paths: &[],
        config_files: &["go.mod"],
        output_format: OutputFormat::Position,
        diagnostics_stream: DiagnosticsStream::Stderr,
        ..ToolSpec::default()
    }
}

pub(crate) fn tsc_like_spec() -> ToolSpec {
    ToolSpec {
        name: "tsc",
        command: &["tsc", "--noEmit", "--pretty", "false"],
        local_paths: &["node_modules/.bin/tsc"],
        config_files: &["tsconfig.json"],
        output_format: OutputFormat::Tsc,
        diagnostics_stream: DiagnosticsStream::Stdout,
        ..ToolSpec::default()
    }
}

pub(crate) fn clippy_like_spec() -> ToolSpec {
    ToolSpec {
        name: "clippy",
        command: &["cargo", "clippy", "--message-format", "json"],
        local_paths: &[],
        config_files: &["Cargo.toml"],
        output_format: OutputFormat::Cargo,
        diagnostics_stream: DiagnosticsStream::Stdout,
        ..ToolSpec::default()
    }
}

/// Re-exported so this suite's `use super::support::*` importers reach the
/// crate-wide helper rather than a local copy.
pub(crate) use crate::test_support::write_executable;

/// The spec enums every literal below and every parser test needs; re-exported
/// for the same reason as `write_executable`.
pub(crate) use crate::languages::spec::{DiagnosticsStream, OutputFormat};

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
        output_format: OutputFormat::Sarif,
        diagnostics_stream: DiagnosticsStream::Stdout,
        ..ToolSpec::default()
    }
}

pub(crate) fn ktlint_like_spec() -> ToolSpec {
    ToolSpec {
        name: "ktlint",
        command: &["ktlint", "--log-level=none", "--reporter=json"],
        local_paths: &[],
        config_files: &[".editorconfig"],
        output_format: OutputFormat::Ktlint,
        diagnostics_stream: DiagnosticsStream::Stdout,
        ..ToolSpec::default()
    }
}

pub(crate) fn shellcheck_like_spec() -> ToolSpec {
    ToolSpec {
        name: "shellcheck",
        command: &["shellcheck", "-f", "json"],
        local_paths: &[],
        config_files: &[".shellcheckrc"],
        output_format: OutputFormat::Shellcheck,
        diagnostics_stream: DiagnosticsStream::Stdout,
        ..ToolSpec::default()
    }
}

pub(crate) fn rubocop_like_spec() -> ToolSpec {
    ToolSpec {
        name: "rubocop",
        command: &["rubocop", "--format", "json", "--force-exclusion"],
        local_paths: &["bin/rubocop"],
        config_files: &[".rubocop.yml"],
        output_format: OutputFormat::Rubocop,
        diagnostics_stream: DiagnosticsStream::Stdout,
        ..ToolSpec::default()
    }
}

pub(crate) fn phpcs_like_spec() -> ToolSpec {
    ToolSpec {
        name: "phpcs",
        command: &["phpcs", "--report=json"],
        local_paths: &["vendor/bin/phpcs"],
        config_files: &["phpcs.xml"],
        output_format: OutputFormat::Phpcs,
        diagnostics_stream: DiagnosticsStream::Stdout,
        ..ToolSpec::default()
    }
}

pub(crate) fn credo_like_spec() -> ToolSpec {
    ToolSpec {
        name: "credo",
        command: &["mix", "credo", "--format", "json"],
        local_paths: &[],
        config_files: &[".credo.exs"],
        output_format: OutputFormat::Credo,
        diagnostics_stream: DiagnosticsStream::Stdout,
        ..ToolSpec::default()
    }
}

pub(crate) fn sqlfluff_like_spec() -> ToolSpec {
    ToolSpec {
        name: "sqlfluff",
        command: &["sqlfluff", "lint", "--format", "json"],
        local_paths: &["venv/bin/sqlfluff"],
        config_files: &[".sqlfluff"],
        output_format: OutputFormat::Sqlfluff,
        diagnostics_stream: DiagnosticsStream::Stdout,
        ..ToolSpec::default()
    }
}

pub(crate) fn dotnet_format_like_spec() -> ToolSpec {
    ToolSpec {
        name: "dotnet format",
        command: &["dotnet", "format", "--verify-no-changes", "--no-restore"],
        local_paths: &[],
        config_files: &["*.csproj"],
        output_format: OutputFormat::Msbuild,
        diagnostics_stream: DiagnosticsStream::Stdout,
        ..ToolSpec::default()
    }
}

pub(crate) fn tflint_like_spec() -> ToolSpec {
    ToolSpec {
        name: "tflint",
        command: &["tflint", "--format", "sarif"],
        local_paths: &[],
        config_files: &[".tflint.hcl"],
        output_format: OutputFormat::Sarif,
        diagnostics_stream: DiagnosticsStream::Stdout,
        ..ToolSpec::default()
    }
}

/// An eslint-shaped spec, for the same reason as [`ruff_like_spec`]: the JSON
/// parser handles two container shapes and both need exercising without
/// eslint installed.
pub(crate) fn eslint_like_spec() -> ToolSpec {
    ToolSpec {
        name: "eslint",
        command: &["eslint"],
        local_paths: &[],
        config_files: &[".eslintrc"],
        output_format: OutputFormat::Json,
        diagnostics_stream: DiagnosticsStream::Stdout,
        ..ToolSpec::default()
    }
}

/// A spec for a tool that reports on the whole project rather than on the
/// files it was handed - the shape `retain_requested` narrows.
///
/// Named for the contract rather than for a real tool, because what the
/// narrowing tests need is `accepts_files: false` and a parser that reads
/// whatever the stub prints; which vendor behaves this way is beside the
/// point.
pub(crate) fn whole_project_lines_spec() -> ToolSpec {
    ToolSpec {
        name: "wholeproject",
        command: &["wholeproject"],
        local_paths: &["wholeproject"],
        config_files: &["marker"],
        output_format: OutputFormat::Lines,
        accepts_files: false,
        ..ToolSpec::default()
    }
}

/// The same spec for a tool that *is* handed its files, so a pair of tests
/// can differ in `accepts_files` alone.
pub(crate) fn per_file_lines_spec() -> ToolSpec {
    ToolSpec {
        name: "perfile",
        command: &["perfile"],
        local_paths: &["perfile"],
        config_files: &["marker"],
        output_format: OutputFormat::Lines,
        ..ToolSpec::default()
    }
}

/// Write the marker `whole_project_lines_spec` looks for and a stub named
/// `tool` that prints `stub` when run, both under `root`.
///
/// Every narrowing and exit-status test opens with this same three-step
/// preamble. `write_executable` rather than `fs::write` + `chmod` is
/// load-bearing on Linux - see its own documentation.
pub(crate) fn install_stub(root: &std::path::Path, tool: &str, stub: &str) {
    std::fs::write(root.join("marker"), "").expect("marker");
    write_executable(&root.join(tool), stub);
}
