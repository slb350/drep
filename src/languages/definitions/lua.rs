//! Lua: luacheck over `.lua`.

use crate::languages::spec::{
    DEFAULT_TOOL_TIMEOUT_SECS, DiagnosticsStream, LanguageSupport, OutputFormat, ToolSpec,
};

/// Lua deterministic checker.
///
/// `--formatter plain` is what produces the `file:line:col:` shape the
/// position parser reads; luacheck's default formatter emits a decorated
/// report the parser skips line by line, leaving a non-empty stream with
/// zero findings and reporting every Lua file `Unavailable`. `--codes`
/// keeps the `(W212)` identifier a user needs to silence a rule, and
/// `--no-color` because the runner captures a pipe and ANSI escapes in a
/// finding message land in the user's terminal.
///
/// `.luacheckrc` is the one config luacheck reads on its own, and a
/// repository without it has not opted into luacheck's defaults.
pub static LUACHECK: ToolSpec = ToolSpec {
    name: "luacheck",
    command: &["luacheck", "--formatter", "plain", "--codes", "--no-color"],
    local_paths: &["lua_modules/bin/luacheck"],
    config_files: &[".luacheckrc"],
    config_flag: None,
    output_format: OutputFormat::Position,
    diagnostics_stream: DiagnosticsStream::Stdout,
    timeout_secs: DEFAULT_TOOL_TIMEOUT_SECS,
    timeout_context: None,
    establishes_compilation: false,
    serial_in_repository: false,
    accepts_files: true,
};

/// Lua language entry.
pub static LUA: LanguageSupport = LanguageSupport {
    name: "lua",
    display_name: "Lua",
    extensions: &[".lua"],
    filenames: &[],
    filename_prefixes: &[],
    tools: &[&LUACHECK],
    conventions: &[
        "Accidental globals from a missing local",
        "nil arithmetic and indexing",
        "1-based indexing and # on tables with holes",
        "Errors swallowed by pcall without inspecting the result",
    ],
    vendored_dirs: &["lua_modules", ".luarocks"],
};

/// The family's entries in registration order. See `ALL_LANGUAGES`.
pub(crate) static FAMILY: &[&LanguageSupport] = &[&LUA];
