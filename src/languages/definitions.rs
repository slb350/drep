//! The registered languages.
//!
//! Adding a language is an entry here plus, if it has one, a tool output parser.
//! No control flow anywhere else in drep changes.
//!
//! `config_files` is what makes a tool run at all: drep checks a project against
//! the style that project has *chosen*, so a repo with no eslint config gets no
//! eslint findings rather than a wall of default-preset complaints.

use super::spec::{LanguageSupport, ToolSpec};

/// Python deterministic checker.
pub static RUFF: ToolSpec = ToolSpec {
    name: "ruff",
    command: &["ruff", "check", "--output-format", "json"],
    local_paths: &["venv/bin/ruff", ".venv/bin/ruff"],
    config_files: &["pyproject.toml", "ruff.toml", ".ruff.toml"],
    output_format: "json",
    diagnostics_stream: "stdout",
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
    output_format: "json",
    diagnostics_stream: "stdout",
};

/// TypeScript's compiler-as-checker. Streams diagnostics to stdout.
pub static TSC: ToolSpec = ToolSpec {
    name: "tsc",
    command: &["tsc", "--noEmit", "--pretty", "false"],
    local_paths: &["node_modules/.bin/tsc"],
    config_files: &["tsconfig.json"],
    output_format: "tsc",
    diagnostics_stream: "stdout",
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
    output_format: "lines",
    diagnostics_stream: "stdout",
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
    output_format: "position",
    diagnostics_stream: "stderr",
};

/// Rust linter - emits structured JSON via cargo's message-format.
pub static CLIPPY: ToolSpec = ToolSpec {
    name: "clippy",
    command: &["cargo", "clippy", "--message-format", "json", "--quiet"],
    local_paths: &[],
    config_files: &["Cargo.toml"],
    output_format: "cargo",
    diagnostics_stream: "stdout",
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

/// Every registered language, in registration order.
pub static ALL_LANGUAGES: &[&LanguageSupport] =
    &[&PYTHON, &JAVASCRIPT, &TYPESCRIPT, &GO, &RUST_LANG];
