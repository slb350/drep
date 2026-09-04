//! The Python ecosystem: ruff over `.py`.

use crate::languages::spec::{
    DEFAULT_TOOL_TIMEOUT_SECS, DiagnosticsStream, LanguageSupport, OutputFormat, ToolSpec,
};

/// Python deterministic checker.
pub static RUFF: ToolSpec = ToolSpec {
    name: "ruff",
    command: &["ruff", "check", "--output-format", "json"],
    local_paths: &["venv/bin/ruff", ".venv/bin/ruff"],
    config_files: &["pyproject.toml", "ruff.toml", ".ruff.toml"],
    config_flag: None,
    output_format: OutputFormat::Json,
    diagnostics_stream: DiagnosticsStream::Stdout,
    timeout_secs: DEFAULT_TOOL_TIMEOUT_SECS,
    timeout_context: None,
    establishes_compilation: false,
    serial_in_repository: false,
    accepts_files: true,
};

/// Python language entry.
pub static PYTHON: LanguageSupport = LanguageSupport {
    name: "python",
    display_name: "Python",
    extensions: &[".py"],
    filenames: &[],
    filename_prefixes: &[],
    tools: &[&RUFF],
    conventions: &[
        "Follows PEP 8 naming and structure",
        "Type hints on public APIs, and correct use of Optional/None",
        "Context managers for resources rather than manual cleanup",
        "Mutable default arguments, and late-binding closures in loops",
    ],
    vendored_dirs: &["__pycache__", "venv", ".venv", "env", ".tox", ".eggs"],
};

/// The family's entries in registration order. See `ALL_LANGUAGES`.
pub(crate) static FAMILY: &[&LanguageSupport] = &[&PYTHON];
