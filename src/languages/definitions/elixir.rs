//! Elixir: Credo over `.ex` and `.exs`.

use crate::languages::spec::{
    DEFAULT_TOOL_TIMEOUT_SECS, DiagnosticsStream, LanguageSupport, OutputFormat, ToolSpec,
};

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
    output_format: OutputFormat::Credo,
    diagnostics_stream: DiagnosticsStream::Stdout,
    timeout_secs: DEFAULT_TOOL_TIMEOUT_SECS,
    timeout_context: None,
    establishes_compilation: false,
    serial_in_repository: false,
    accepts_files: true,
};

/// Elixir language entry.
///
/// `deps` is deliberately absent from `vendored_dirs` for the reason the JVM
/// family leaves out `out`: `files::is_ignored_dir` consults the union across
/// every language, so an entry here would skip `deps/` in repositories with
/// no Elixir in them at all - and a checked-in `deps/` of real source is a
/// convention in other ecosystems. Mix projects gitignore `/deps` in
/// practice, which the walker already honors on its own.
pub static ELIXIR: LanguageSupport = LanguageSupport {
    name: "elixir",
    display_name: "Elixir",
    extensions: &[".ex", ".exs"],
    filenames: &[],
    filename_prefixes: &[],
    tools: &[&CREDO],
    conventions: &[
        "Pattern matches with no fallback clause, crashing on unexpected shapes",
        "Non-tail recursion over unbounded lists",
        "Atoms built from untrusted input",
        "GenServer calls that deadlock against themselves",
        "Unhandled messages accumulating in a mailbox",
    ],
    vendored_dirs: &["_build"],
};

/// The family's entries in registration order. See `ALL_LANGUAGES`.
pub(crate) static FAMILY: &[&LanguageSupport] = &[&ELIXIR];
