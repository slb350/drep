//! The Go ecosystem: gofmt and go vet over `.go`.

use crate::languages::spec::{DEFAULT_TOOL_TIMEOUT_SECS, LanguageSupport, ToolSpec};

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

/// Go language entry.
pub static GO: LanguageSupport = LanguageSupport {
    name: "go",
    display_name: "Go",
    extensions: &[".go"],
    filenames: &[],
    tools: &[&GOFMT, &GO_VET],
    conventions: &[
        "Errors ignored rather than checked and wrapped",
        "Goroutine leaks, and writes to a channel nobody reads",
        "defer inside a loop, and defer that never runs",
        "Data races on shared state without synchronisation",
    ],
    vendored_dirs: &["vendor"],
};
