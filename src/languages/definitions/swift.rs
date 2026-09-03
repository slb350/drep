//! Swift: SwiftLint over `.swift`.

use crate::languages::spec::{DEFAULT_TOOL_TIMEOUT_SECS, LanguageSupport, ToolSpec};

/// Swift deterministic checker.
///
/// `--quiet` suppresses the "Linting Swift files" banner so stdout is the
/// SARIF document alone. SwiftLint's SARIF `uri` is repo-relative, verified
/// against 0.65.1, so it needs none of the `file:` URI handling checkstyle's
/// does.
pub static SWIFTLINT: ToolSpec = ToolSpec {
    name: "swiftlint",
    command: &["swiftlint", "lint", "--reporter", "sarif", "--quiet"],
    local_paths: &[],
    config_files: &[".swiftlint.yml"],
    config_flag: None,
    output_format: "sarif",
    diagnostics_stream: "stdout",
    timeout_secs: DEFAULT_TOOL_TIMEOUT_SECS,
    timeout_context: None,
    establishes_compilation: false,
    serial_in_repository: false,
    accepts_files: true,
};

/// Swift language entry.
pub static SWIFT: LanguageSupport = LanguageSupport {
    name: "swift",
    display_name: "Swift",
    extensions: &[".swift"],
    filenames: &[],
    tools: &[&SWIFTLINT],
    conventions: &[
        "Force unwraps and force tries on values that can be nil or throw",
        "Retain cycles from closures capturing self strongly",
        "Unowned references that outlive their target",
        "Main-actor isolation violated from a synchronous context",
    ],
    vendored_dirs: &[".build", "Pods", "DerivedData"],
};
