//! The JVM family: Java, Kotlin and Scala, plus Groovy build scripts.

use crate::languages::spec::{DEFAULT_TOOL_TIMEOUT_SECS, LanguageSupport, ToolSpec};

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

/// Java language entry.
pub static JAVA: LanguageSupport = LanguageSupport {
    name: "java",
    display_name: "Java",
    extensions: &[".java"],
    filenames: &[],
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
    filenames: &[],
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
    filenames: &[],
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
    filenames: &[],
    tools: &[],
    conventions: &[
        "Dynamic dispatch where a typed call would fail at compile time",
        "Gradle configuration-time work that belongs in a task action",
        "Dependency and plugin versions pinned versus floating",
        "String interpolation of values that should be escaped",
    ],
    vendored_dirs: JVM_VENDORED_DIRS,
};
