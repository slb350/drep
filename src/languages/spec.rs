//! Language support contract shared by every registered language.
//!
//! drep analyzes a file in two layers, and this module is what keeps them free of
//! per-language conditionals:
//!
//! - **Deterministic**: the project's own tools (ruff, eslint, gofmt, clippy).
//!   They are precise, so their findings can gate a commit.
//! - **Semantic**: the LLM, told which language it is looking at. It reads any
//!   language without a parser, so it needs no per-language machinery beyond a
//!   prompt - which is why adding a language here is a data change, not a
//!   refactor.
//!
//! Deliberately free of heavyweight drep imports: the registry is consulted by
//! file discovery (`drep.core.file_targets`), which analyzer packages import.

/// A deterministic checker for one language.
///
/// Attributes:
///     name: Tool name, used in logs and finding provenance.
///     command: argv to run, minus the files. The first element is resolved
///         against local_paths before PATH.
///     local_paths: Repo-relative locations to prefer over PATH, so a project
///         gets the version its own CI runs (node_modules/.bin/eslint rather
///         than whatever is installed globally).
///     config_files: Repo-relative paths that mean "this project has opted
///         into this tool". A tool with none of them present is skipped: its
///         defaults are not the project's chosen style, so running it anyway
///         would invent findings the project never asked for.
///     output_format: How to parse the tool's diagnostics into findings.
///     diagnostics_stream: Which stream carries them. `go vet` writes to
///         stderr, so reading only stdout would report every Go file clean.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolSpec {
    /// Tool name, used in logs and finding provenance.
    pub name: &'static str,
    /// argv to run, minus the files. The first element is resolved against `local_paths` before PATH.
    pub command: &'static [&'static str],
    /// Repo-relative locations to prefer over PATH, so a project gets the
    /// version its own CI runs (`node_modules/.bin/eslint` rather than
    /// whatever is installed globally).
    pub local_paths: &'static [&'static str],
    /// Repo-relative paths that mean "this project has opted into this tool".
    /// A tool with none of them present is skipped: its defaults are not the
    /// project's chosen style, so running it anyway would invent findings the
    /// project never asked for.
    pub config_files: &'static [&'static str],
    /// How to parse the tool's diagnostics into findings.
    pub output_format: &'static str,
    /// Which stream carries them. `go vet` writes to stderr, so reading only
    /// stdout would report every Go file clean.
    pub diagnostics_stream: &'static str,
}

impl Default for ToolSpec {
    fn default() -> Self {
        Self {
            name: "",
            command: &[],
            local_paths: &[],
            config_files: &[],
            output_format: "json",
            diagnostics_stream: "stdout",
        }
    }
}

/// Everything drep needs to know about one language.
///
/// Attributes:
///     name: Registry key (lowercase, e.g. "typescript").
///     display_name: How the language is named to the LLM and to users.
///     extensions: Lowercased suffixes this language owns, including the dot.
///     tools: Deterministic checkers, in the order they should run.
///     conventions: Language-specific guidance appended to the analysis
///         prompt - the part that used to be hardcoded as PEP 8.
///     vendored_dirs: Dependency and build directories this language creates,
///         never descended into. Declared here rather than in a global list
///         so adding a language stays a single-file change.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct LanguageSupport {
    /// Registry key (lowercase, e.g. `"typescript"`).
    pub name: &'static str,
    /// How the language is named to the LLM and to users.
    pub display_name: &'static str,
    /// Lowercased suffixes this language owns, including the dot.
    pub extensions: &'static [&'static str],
    /// Deterministic checkers, in the order they should run.
    pub tools: &'static [&'static ToolSpec],
    /// Language-specific guidance appended to the analysis prompt - the part
    /// that used to be hardcoded as PEP 8.
    pub conventions: &'static [&'static str],
    /// Dependency and build directories this language creates, never
    /// descended into. Declared here rather than in a global list so adding
    /// a language stays a single-file change.
    pub vendored_dirs: &'static [&'static str],
}
