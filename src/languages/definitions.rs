//! The registered languages.
//!
//! Adding a language is an entry here plus, if it has one, a tool output parser.
//! No control flow anywhere else in drep changes.
//!
//! `config_files` is what makes a tool run at all: drep checks a project against
//! the style that project has *chosen*, so a repo with no eslint config gets no
//! eslint findings rather than a wall of default-preset complaints.

use super::spec::{DEFAULT_TOOL_TIMEOUT_SECS, LanguageSupport, ToolSpec};

/// Build outputs shared by the JVM languages. Gradle writes `build` and
/// `.gradle`, Maven writes `target`. Declared once rather than repeated across
/// four entries that can never legitimately disagree.
///
/// The set is global in effect: `files::is_ignored_dir` consults the union of
/// every language's vendored directories, so an entry here skips the directory
/// in a repository with no JVM code at all. `out` is therefore deliberately
/// absent: IntelliJ's build output is nearly always gitignored anyway (which
/// the walker honors on its own), while the name is generic enough that a
/// checked-in `out/` of real sources in some other ecosystem would be silently
/// dropped from review.
static JVM_VENDORED_DIRS: &[&str] = &["build", ".gradle", "target"];

/// Python deterministic checker.
pub static RUFF: ToolSpec = ToolSpec {
    name: "ruff",
    command: &["ruff", "check", "--output-format", "json"],
    local_paths: &["venv/bin/ruff", ".venv/bin/ruff"],
    config_files: &["pyproject.toml", "ruff.toml", ".ruff.toml"],
    config_flag: None,
    output_format: "json",
    diagnostics_stream: "stdout",
    timeout_secs: DEFAULT_TOOL_TIMEOUT_SECS,
    timeout_context: None,
    establishes_compilation: false,
    serial_in_repository: false,
    accepts_files: true,
};

/// JavaScript deterministic checker.
pub static ESLINT: ToolSpec = ToolSpec {
    name: "eslint",
    command: &["eslint", "--format", "json"],
    local_paths: &["node_modules/.bin/eslint"],
    config_files: &[
        "eslint.config.js",
        "eslint.config.mjs",
        "eslint.config.cjs",
        ".eslintrc",
        ".eslintrc.js",
        ".eslintrc.cjs",
        ".eslintrc.json",
        ".eslintrc.yml",
        ".eslintrc.yaml",
    ],
    config_flag: None,
    output_format: "json",
    diagnostics_stream: "stdout",
    timeout_secs: DEFAULT_TOOL_TIMEOUT_SECS,
    timeout_context: None,
    establishes_compilation: false,
    serial_in_repository: false,
    accepts_files: true,
};

/// TypeScript's compiler-as-checker. Streams diagnostics to stdout.
pub static TSC: ToolSpec = ToolSpec {
    name: "tsc",
    command: &["tsc", "--noEmit", "--pretty", "false"],
    local_paths: &["node_modules/.bin/tsc"],
    config_files: &["tsconfig.json"],
    config_flag: None,
    output_format: "tsc",
    diagnostics_stream: "stdout",
    timeout_secs: DEFAULT_TOOL_TIMEOUT_SECS,
    timeout_context: None,
    establishes_compilation: true,
    serial_in_repository: false,
    // Passing source files makes tsc ignore tsconfig.json. Run the configured
    // project and filter its diagnostics back to the requested files.
    accepts_files: false,
};

/// Go formatting checker - lists files whose formatting drifts from `gofmt`'s.
///
/// `go.mod` is the marker that this is a Go module at all; gofmt has no
/// config of its own because its formatting is not configurable.
pub static GOFMT: ToolSpec = ToolSpec {
    name: "gofmt",
    command: &["gofmt", "-l"],
    local_paths: &[],
    config_files: &["go.mod"],
    config_flag: None,
    output_format: "lines",
    diagnostics_stream: "stdout",
    timeout_secs: DEFAULT_TOOL_TIMEOUT_SECS,
    timeout_context: None,
    establishes_compilation: false,
    serial_in_repository: false,
    accepts_files: true,
};

/// `go vet`. Streams diagnostics to stderr.
///
/// Not -json: that only emits JSON once the package compiles, and a package
/// that does not compile is exactly when vet has the most to say.
pub static GO_VET: ToolSpec = ToolSpec {
    name: "go vet",
    command: &["go", "vet"],
    local_paths: &[],
    config_files: &["go.mod"],
    config_flag: None,
    output_format: "position",
    diagnostics_stream: "stderr",
    timeout_secs: DEFAULT_TOOL_TIMEOUT_SECS,
    timeout_context: None,
    establishes_compilation: true,
    serial_in_repository: false,
    accepts_files: true,
};

/// Rust linter - emits structured JSON via cargo's message-format.
pub static CLIPPY: ToolSpec = ToolSpec {
    name: "clippy",
    command: &["cargo", "clippy", "--message-format", "json", "--quiet"],
    local_paths: &[],
    config_files: &["Cargo.toml"],
    config_flag: None,
    output_format: "cargo",
    diagnostics_stream: "stdout",
    // Cargo's build lock is acquired by Cargo itself, so its wait is part of
    // the child process. Allow the same long-running ceiling as an LLM review
    // rather than failing a whole gate at the generic two-minute tool limit.
    timeout_secs: 1_800,
    timeout_context: Some(", including its Cargo build-lock wait"),
    establishes_compilation: true,
    serial_in_repository: true,
    // `cargo clippy` checks a crate, not files: a path argument is rejected
    // with "unexpected argument". See `ToolSpec::accepts_files`.
    accepts_files: false,
};

/// Java linter. Emits SARIF 2.1.0 on stdout; its startup banner goes to stderr.
///
/// `-c` is not in `command`: checkstyle refuses to run without a ruleset and
/// which one a project uses is exactly what `config_files` discovers, so the
/// path is appended by `config_flag`. Bare, it exits 1 with "Must specify a
/// config XML".
pub static CHECKSTYLE: ToolSpec = ToolSpec {
    name: "checkstyle",
    command: &["checkstyle", "-f", "sarif"],
    local_paths: &[],
    config_files: &[
        "checkstyle.xml",
        ".checkstyle.xml",
        "config/checkstyle/checkstyle.xml",
        "gradle/config/checkstyle/checkstyle.xml",
    ],
    config_flag: Some("-c"),
    output_format: "sarif",
    diagnostics_stream: "stdout",
    // A JVM start plus a full reflections scan of the check registry, on every
    // invocation. Two minutes is enough but not generous on a cold page cache.
    timeout_secs: DEFAULT_TOOL_TIMEOUT_SECS,
    timeout_context: None,
    // It parses; it does not compile. A clean run says nothing about whether
    // javac would accept the file.
    establishes_compilation: false,
    serial_in_repository: false,
    accepts_files: true,
};

/// Kotlin linter and formatter, run in lint-only mode.
///
/// `--log-level=none` is load-bearing: ktlint writes "Lint has found errors
/// than can be autocorrected" to **stdout**, ahead of the JSON, and the parser
/// would reject the whole run as unparseable rather than report the findings.
pub static KTLINT: ToolSpec = ToolSpec {
    name: "ktlint",
    command: &["ktlint", "--log-level=none", "--reporter=json"],
    local_paths: &[],
    // ktlint reads `.editorconfig` and nothing else. A Kotlin repo without one
    // has not chosen ktlint's defaults, so it is skipped.
    config_files: &[".editorconfig"],
    config_flag: None,
    output_format: "ktlint",
    diagnostics_stream: "stdout",
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
    tools: &[&RUFF],
    conventions: &[
        "Follows PEP 8 naming and structure",
        "Type hints on public APIs, and correct use of Optional/None",
        "Context managers for resources rather than manual cleanup",
        "Mutable default arguments, and late-binding closures in loops",
    ],
    vendored_dirs: &["__pycache__", "venv", ".venv", "env", ".tox", ".eggs"],
};

/// JavaScript language entry.
pub static JAVASCRIPT: LanguageSupport = LanguageSupport {
    name: "javascript",
    display_name: "JavaScript",
    extensions: &[".js", ".jsx", ".mjs", ".cjs"],
    tools: &[&ESLINT],
    conventions: &[
        "Unhandled promise rejections and missing await",
        "Sequential awaits in a loop where the work is independent",
        "var versus let/const, and accidental global scope",
        "Equality coercion (== versus ===)",
    ],
    vendored_dirs: &["node_modules", ".next", ".nuxt"],
};

/// TypeScript language entry.
pub static TYPESCRIPT: LanguageSupport = LanguageSupport {
    name: "typescript",
    display_name: "TypeScript",
    extensions: &[".ts", ".tsx", ".mts", ".cts"],
    tools: &[&ESLINT, &TSC],
    conventions: &[
        "`any` where a real type is available, and unsafe casts",
        "Unhandled promise rejections and missing await",
        "Non-null assertions (!) that hide a genuine null case",
        "Sequential awaits in a loop where the work is independent",
    ],
    vendored_dirs: &["node_modules", ".next", ".nuxt"],
};

/// Go language entry.
pub static GO: LanguageSupport = LanguageSupport {
    name: "go",
    display_name: "Go",
    extensions: &[".go"],
    tools: &[&GOFMT, &GO_VET],
    conventions: &[
        "Errors ignored rather than checked and wrapped",
        "Goroutine leaks, and writes to a channel nobody reads",
        "defer inside a loop, and defer that never runs",
        "Data races on shared state without synchronisation",
    ],
    vendored_dirs: &["vendor"],
};

/// Rust language entry.
pub static RUST_LANG: LanguageSupport = LanguageSupport {
    name: "rust",
    display_name: "Rust",
    extensions: &[".rs"],
    tools: &[&CLIPPY],
    conventions: &[
        "unwrap/expect on values that can legitimately be None or Err",
        "unsafe blocks, and whether their invariants are documented",
        "Unnecessary clones and allocations in hot paths",
        "Send/Sync correctness for types crossing threads",
    ],
    vendored_dirs: &["target"],
};

/// Java language entry.
pub static JAVA: LanguageSupport = LanguageSupport {
    name: "java",
    display_name: "Java",
    extensions: &[".java"],
    tools: &[&CHECKSTYLE],
    conventions: &[
        "Resources closed on every path, and try-with-resources where it applies",
        "Null handling: Optional versus a nullable return, and unchecked dereferences",
        "equals/hashCode/compareTo consistency, and mutable state in a shared object",
        "Exceptions swallowed or logged and rethrown, losing the original cause",
        "Concurrency: unsynchronised shared state, and non-thread-safe fields on a singleton",
    ],
    vendored_dirs: JVM_VENDORED_DIRS,
};

/// Kotlin language entry.
///
/// `.kts` covers both scratch scripts and `build.gradle.kts`, which
/// `Path::extension` reports as `kts` rather than `gradle.kts`.
pub static KOTLIN: LanguageSupport = LanguageSupport {
    name: "kotlin",
    display_name: "Kotlin",
    extensions: &[".kt", ".kts"],
    tools: &[&KTLINT],
    conventions: &[
        "Platform types from Java interop dereferenced without a null check",
        "runBlocking on a coroutine path, and scopes that outlive their work",
        "!! where the null case is real, and lateinit read before assignment",
        "data class equality over mutable properties",
    ],
    vendored_dirs: JVM_VENDORED_DIRS,
};

/// Scala language entry.
///
/// No deterministic tool. scalafmt and scalafix are both build-plugin-first
/// here, and neither has a standalone CLI drep can invoke the way it invokes
/// ruff. The semantic half needs none, so the language is registered anyway
/// rather than leaving `.scala` unreadable.
pub static SCALA: LanguageSupport = LanguageSupport {
    name: "scala",
    display_name: "Scala",
    extensions: &[".scala", ".sc"],
    tools: &[],
    conventions: &[
        "Partial functions and non-exhaustive matches",
        "Option/Either handling versus get and head on an empty collection",
        "Implicits whose resolution is not obvious at the call site",
        "Futures without an explicit ExecutionContext, and blocking inside one",
    ],
    vendored_dirs: JVM_VENDORED_DIRS,
};

/// Groovy language entry.
///
/// `.gradle` is here because a Gradle build script is Groovy, and a change to
/// one is exactly the kind of thing worth a second read. No deterministic
/// tool: CodeNarc is a build plugin rather than a CLI.
pub static GROOVY: LanguageSupport = LanguageSupport {
    name: "groovy",
    display_name: "Groovy",
    extensions: &[".groovy", ".gradle"],
    tools: &[],
    conventions: &[
        "Dynamic dispatch where a typed call would fail at compile time",
        "Gradle configuration-time work that belongs in a task action",
        "Dependency and plugin versions pinned versus floating",
        "String interpolation of values that should be escaped",
    ],
    vendored_dirs: JVM_VENDORED_DIRS,
};

/// Every registered language, in registration order.
pub static ALL_LANGUAGES: &[&LanguageSupport] = &[
    &PYTHON,
    &JAVASCRIPT,
    &TYPESCRIPT,
    &GO,
    &RUST_LANG,
    &JAVA,
    &KOTLIN,
    &SCALA,
    &GROOVY,
];
