//! SQL: sqlfluff over `.sql`.

use crate::languages::spec::{DEFAULT_TOOL_TIMEOUT_SECS, LanguageSupport, ToolSpec};

/// SQL deterministic checker.
pub static SQLFLUFF: ToolSpec = ToolSpec {
    name: "sqlfluff",
    command: &["sqlfluff", "lint", "--format", "json"],
    local_paths: &["venv/bin/sqlfluff", ".venv/bin/sqlfluff"],
    config_files: &[".sqlfluff"],
    config_flag: None,
    output_format: "sqlfluff",
    diagnostics_stream: "stdout",
    timeout_secs: DEFAULT_TOOL_TIMEOUT_SECS,
    timeout_context: None,
    establishes_compilation: false,
    serial_in_repository: false,
    accepts_files: true,
};

/// SQL language entry.
///
/// `.sql` covers hand-written migrations, stored procedures and query files
/// alike, all of which sqlfluff reads.
pub static SQL: LanguageSupport = LanguageSupport {
    name: "sql",
    display_name: "SQL",
    extensions: &[".sql"],
    filenames: &[],
    tools: &[&SQLFLUFF],
    conventions: &[
        "Queries that scan a whole table where an index exists",
        "Joins missing a predicate, and accidental cross joins",
        "NULL equality filtering out the rows it meant to match",
        "Migrations that lock or rewrite a large table",
        "Ordering without a tiebreaker that pages nondeterministically",
    ],
    vendored_dirs: &[],
};
