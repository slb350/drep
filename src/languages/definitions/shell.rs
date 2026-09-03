//! Shell: ShellCheck over `.sh` and `.bash`.

use crate::languages::spec::{DEFAULT_TOOL_TIMEOUT_SECS, LanguageSupport, ToolSpec};

/// Shell deterministic checker.
///
/// `.shellcheckrc` is the one config ShellCheck reads, and a repository
/// without it has not opted into ShellCheck's defaults.
pub static SHELLCHECK: ToolSpec = ToolSpec {
    name: "shellcheck",
    command: &["shellcheck", "-f", "json"],
    local_paths: &[],
    config_files: &[".shellcheckrc"],
    config_flag: None,
    output_format: "shellcheck",
    diagnostics_stream: "stdout",
    timeout_secs: DEFAULT_TOOL_TIMEOUT_SECS,
    timeout_context: None,
    establishes_compilation: false,
    serial_in_repository: false,
    accepts_files: true,
};

/// Shell language entry.
pub static SHELL: LanguageSupport = LanguageSupport {
    name: "shell",
    display_name: "Shell",
    extensions: &[".sh", ".bash"],
    filenames: &[],
    tools: &[&SHELLCHECK],
    conventions: &[
        "Unquoted expansions that glob or word-split",
        "Commands whose failure is never checked",
        "Pipelines whose failure set -e does not catch",
        "cd without checking, and traps that never fire on the failure path",
    ],
    vendored_dirs: &[],
};
