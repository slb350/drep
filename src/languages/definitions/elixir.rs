//! Elixir: Credo over `.ex` and `.exs`.

use crate::languages::spec::{DEFAULT_TOOL_TIMEOUT_SECS, LanguageSupport, ToolSpec};

/// Elixir deterministic checker.
///
/// `mix credo` runs from the project root where `mix.exs` lives; `.credo.exs`
/// is the config marker, present when the project has tuned (or even merely
/// accepted) Credo's checks.
pub static CREDO: ToolSpec = ToolSpec {
    name: "credo",
    command: &["mix", "credo", "--format", "json"],
    local_paths: &[],
    config_files: &[".credo.exs"],
    config_flag: None,
    output_format: "credo",
    diagnostics_stream: "stdout",
    timeout_secs: DEFAULT_TOOL_TIMEOUT_SECS,
    timeout_context: None,
    establishes_compilation: false,
    serial_in_repository: false,
    accepts_files: true,
};

/// Elixir language entry.
pub static ELIXIR: LanguageSupport = LanguageSupport {
    name: "elixir",
    display_name: "Elixir",
    extensions: &[".ex", ".exs"],
    filenames: &[],
    tools: &[&CREDO],
    conventions: &[
        "Pattern matches with no fallback clause, crashing on unexpected shapes",
        "Non-tail recursion over unbounded lists",
        "Atoms built from untrusted input",
        "GenServer calls that deadlock against themselves",
        "Unhandled messages accumulating in a mailbox",
    ],
    vendored_dirs: &["_build", "deps"],
};
