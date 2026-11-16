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

pub const DEFAULT_TOOL_TIMEOUT_SECS: u64 = 120;

/// How to parse the tool's diagnostics into findings.
///
/// A fieldless enum rather than a string: `parse_output` dispatches on it, so
/// a misspelling is a compile error at the definition site instead of a
/// runtime "no parser for output format" surfacing only when the tool runs.
/// No `#[non_exhaustive]` - this crate is the only consumer, and the
/// wildcard arm it forces is exactly the silent-fallback hole the enum
/// exists to close.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Lines,
    Json,
    Position,
    Tsc,
    Cargo,
    Sarif,
    Ktlint,
    Shellcheck,
    Rubocop,
    Phpcs,
    Credo,
    Sqlfluff,
    Msbuild,
}

impl OutputFormat {
    /// Whether this format's parser *skips* input it does not recognise
    /// instead of erroring on it.
    ///
    /// The skip parsers exist because their tools interleave chatter among
    /// the diagnostics (Go's `# pkg` headers, MSBuild's restore noise). The
    /// price is that a run whose every line is chatter of a new shape - an
    /// SDK error, a rejected invocation - parses as zero findings on a
    /// non-empty stream, which is why the runner treats that combination as
    /// `Unavailable` for these formats and no others. Every other format
    /// errors on input it cannot parse, so the hole cannot open there.
    pub fn skips_unmatched_input(self) -> bool {
        matches!(self, Self::Position | Self::Tsc | Self::Msbuild)
    }
}

/// Which process stream carries a tool's diagnostics.
///
/// `go vet` writes to stderr, so reading only stdout would report every Go
/// file clean. An enum because the runner compares it exactly once: a typo'd
/// string silently selected stdout, which is that same failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticsStream {
    Stdout,
    Stderr,
}

/// What a tool is handed on its command line after its own flags.
///
/// Anything but [`ToolArguments::Files`] reports on more than the files being checked, so its findings are narrowed back to them. Without that a commit gate would block on pre-existing issues in code the commit never touched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolArguments {
    /// The files being checked.
    Files,
    /// The Go package directory each file being checked sits in, each once, as `./dir`, after `go list` has said which of the files the build includes.
    ///
    /// `go vet` checks a package, and a Go package is a directory. Handed files, it type-checks them as a package of their own: a diff touching one file of a multi-file package reported every symbol its siblings declare as `undefined`, and a diff spanning two directories failed with "named files must all be in one directory". The `./` prefix is what makes `go` read the argument as a path rather than an import path.
    ///
    /// A directory is all-or-nothing for the whole call, so `go list -e -json` runs first. A file build constraints exclude (`//go:build tools`, `x_windows.go` on a Mac) is reported in [`ToolOutcome::excluded`](crate::languages::runner::ToolOutcome::excluded) and its directory is left out: `go vet ./tools` on a directory with no buildable file fails, and that failure aborts every other package in the call. A package that fails to load, an unresolvable import say, aborts the call the same way, so its load errors become the findings and vet does not run.
    GoPackages,
    /// Nothing: the tool checks its whole project.
    ///
    /// `cargo clippy` checks a *crate*, and a path argument is rejected outright with "unexpected argument". Appending files to it therefore made every run fail, so every Rust file came back `Unavailable` and `drep check` exited 2 on any Rust repository - the deterministic half for Rust never ran at all.
    Project,
}

/// A deterministic checker for one language.
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
    /// Flag that hands the discovered config file to the tool, e.g. `"-c"`.
    ///
    /// Most checkers find their own config: ruff reads `pyproject.toml` out of
    /// the working directory without being told. The JVM linters do not.
    /// `checkstyle` run bare exits 1 with "Must specify a config XML", so
    /// without this it could not run at all. When set, the config path
    /// `config_files` already discovered is appended as
    /// `[config_flag, <path>]` ahead of the file arguments.
    pub config_flag: Option<&'static str>,
    /// How to parse the tool's diagnostics into findings.
    pub output_format: OutputFormat,
    /// Which stream carries them.
    pub diagnostics_stream: DiagnosticsStream,
    /// Wall-clock ceiling for the process. This includes any tool-owned lock
    /// wait before analysis starts.
    pub timeout_secs: u64,
    /// Optional diagnostic suffix explaining a legitimate long wait.
    pub timeout_context: Option<&'static str>,
    /// A zero-exit run proves the checked files compiled/typechecked.
    pub establishes_compilation: bool,
    /// Whether invocations of this tool must be serialized within a repo.
    pub serial_in_repository: bool,
    /// What the tool is handed after its own flags. See [`ToolArguments`].
    pub arguments: ToolArguments,
    /// Whether the tool, given files, may also read other files: an import it
    /// follows (eslint, `go vet`), a sourced script (ShellCheck), a package
    /// marker whose presence it checks (ruff's `__init__.py`), a schema a
    /// plugin reads (rubocop-rails' `db/schema.rb`), or code its configuration
    /// loads (a `.luacheckrc` is a Lua script that can `require` more, and a
    /// PHPCS ruleset can name custom sniffs).
    ///
    /// In staged mode such a tool is refused while any file in the
    /// repository is uncommitted, since it could read content the commit does
    /// not hold: an import or a sourced helper can sit outside its workspace,
    /// under any name, and in another language drep registers. Only a tool that reads nothing but the files it
    /// is given, as `gofmt` formats each one alone, says false; saying true
    /// of one that does not only costs a refusal, while saying false of one
    /// that does lets an uncommitted file decide a commit's lint. A tool not handed exactly its files ([`ToolArguments::GoPackages`] or [`ToolArguments::Project`]) reads beyond them by definition and must say true, which the registry's tests hold.
    pub reads_other_sources: bool,
}

impl Default for ToolSpec {
    fn default() -> Self {
        Self {
            name: "",
            command: &[],
            local_paths: &[],
            config_files: &[],
            config_flag: None,
            output_format: OutputFormat::Json,
            diagnostics_stream: DiagnosticsStream::Stdout,
            timeout_secs: DEFAULT_TOOL_TIMEOUT_SECS,
            timeout_context: None,
            establishes_compilation: false,
            serial_in_repository: false,
            arguments: ToolArguments::Files,
            reads_other_sources: true,
        }
    }
}

/// Everything drep needs to know about one language.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct LanguageSupport {
    /// Registry key (lowercase, e.g. `"typescript"`).
    pub name: &'static str,
    /// How the language is named to the LLM and to users.
    pub display_name: &'static str,
    /// Lowercased suffixes this language owns, including the dot.
    pub extensions: &'static [&'static str],
    /// Whole file names this language owns, for files that carry no extension
    /// at all.
    ///
    /// `Path::extension` returns `None` for `Dockerfile`, `Gemfile` and
    /// `Rakefile`, so an extension-only registry drops them before either
    /// analysis layer sees them - the same silent pass that unregistered
    /// `.java` produced, on files most repositories have. Only names a
    /// registered language actually claims belong here: `Makefile` and
    /// `Jenkinsfile` are deliberately absent, because no language claims them
    /// and a name with no owner would still resolve to nothing.
    /// Matched only when the extension lookup finds nothing, so a name here
    /// can never shadow a language that claims the suffix.
    pub filenames: &'static [&'static str],
    /// Stems that additionally claim their dotted variants, for families like
    /// `Dockerfile.dev`.
    ///
    /// `filenames` claims the exact canonical name; a stem here claims
    /// `<stem>.<anything>` as well. Multi-image layouts name per-environment
    /// Dockerfiles `Dockerfile.dev`, `Dockerfile.prod`, `Dockerfile.web` - an
    /// unbounded family an exact list cannot cover, and hadolint lints the
    /// variants the same as the canonical file. The extension lookup runs
    /// first, so `Dockerfile.ts` remains TypeScript, and the variant must
    /// carry a non-empty suffix after the dot, so `Dockerfile.` itself is
    /// not claimed.
    pub filename_prefixes: &'static [&'static str],
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
