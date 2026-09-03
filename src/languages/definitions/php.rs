//! PHP: PHP_CodeSniffer over `.php`.

use crate::languages::spec::{DEFAULT_TOOL_TIMEOUT_SECS, LanguageSupport, ToolSpec};

/// PHP deterministic checker.
pub static PHPCS: ToolSpec = ToolSpec {
    name: "phpcs",
    command: &["phpcs", "--report=json"],
    local_paths: &["vendor/bin/phpcs"],
    config_files: &[
        "phpcs.xml",
        "phpcs.xml.dist",
        ".phpcs.xml",
        ".phpcs.xml.dist",
    ],
    config_flag: None,
    output_format: "phpcs",
    diagnostics_stream: "stdout",
    timeout_secs: DEFAULT_TOOL_TIMEOUT_SECS,
    timeout_context: None,
    establishes_compilation: false,
    serial_in_repository: false,
    accepts_files: true,
};

/// PHP language entry.
pub static PHP: LanguageSupport = LanguageSupport {
    name: "php",
    display_name: "PHP",
    extensions: &[".php"],
    filenames: &[],
    tools: &[&PHPCS],
    conventions: &[
        "Undefined variables and array keys relied on as null",
        "Type juggling in == comparisons",
        "SQL and shell commands built by string interpolation",
        "Unescaped output of user input in templates",
    ],
    vendored_dirs: &["vendor"],
};
