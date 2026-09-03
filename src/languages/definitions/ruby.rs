//! Ruby: RuboCop over `.rb` and the extensionless `Gemfile`/`Rakefile`.

use crate::languages::spec::{DEFAULT_TOOL_TIMEOUT_SECS, LanguageSupport, ToolSpec};

/// Ruby deterministic checker.
///
/// `--force-exclusion` makes RuboCop honor a repo's `Exclude` lists for
/// explicitly-named files; without it, being handed a path on the command
/// line overrides the config's own exclusions and reports files the project
/// deliberately does not lint.
pub static RUBOCOP: ToolSpec = ToolSpec {
    name: "rubocop",
    command: &["rubocop", "--format", "json", "--force-exclusion"],
    local_paths: &["bin/rubocop"],
    config_files: &[".rubocop.yml"],
    config_flag: None,
    output_format: "rubocop",
    diagnostics_stream: "stdout",
    timeout_secs: DEFAULT_TOOL_TIMEOUT_SECS,
    timeout_context: None,
    establishes_compilation: false,
    serial_in_repository: false,
    accepts_files: true,
};

/// Ruby language entry.
///
/// `Gemfile` and `Rakefile` carry no extension, so `filenames` claims them:
/// a dependency change in a Gemfile is exactly the kind of change worth a
/// deterministic read, and an extension-only registry drops them.
pub static RUBY: LanguageSupport = LanguageSupport {
    name: "ruby",
    display_name: "Ruby",
    extensions: &[".rb", ".rake", ".gemspec"],
    filenames: &["Gemfile", "Rakefile"],
    tools: &[&RUBOCOP],
    conventions: &[
        "Monkey patches and reopenings of core classes",
        "nil flowing where the code assumes a value",
        "Mutable defaults shared across calls",
        "Exceptions swallowed by a rescue that only logs",
        "Blocks whose break/next semantics skip cleanup",
    ],
    vendored_dirs: &["vendor/bundle", ".bundle"],
};
