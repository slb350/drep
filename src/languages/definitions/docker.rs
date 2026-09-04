//! Docker: hadolint over `Dockerfile` and `Containerfile`.

use crate::languages::spec::{
    DEFAULT_TOOL_TIMEOUT_SECS, DiagnosticsStream, LanguageSupport, OutputFormat, ToolSpec,
};

/// Docker deterministic checker.
///
/// hadolint's SARIF `uri` is repo-relative, verified against the real binary.
pub static HADOLINT: ToolSpec = ToolSpec {
    name: "hadolint",
    command: &["hadolint", "--format", "sarif"],
    local_paths: &[],
    config_files: &[".hadolint.yaml", ".hadolint.yml"],
    config_flag: None,
    output_format: OutputFormat::Sarif,
    diagnostics_stream: DiagnosticsStream::Stdout,
    timeout_secs: DEFAULT_TOOL_TIMEOUT_SECS,
    timeout_context: None,
    establishes_compilation: false,
    serial_in_repository: false,
    accepts_files: true,
};

/// Docker language entry.
///
/// Dockerfiles carry no extension, so `filenames` claims both conventional
/// spellings; `.dockerfile` is also claimed for the projects that do use it
/// as one. The stems claim the per-environment variants - `Dockerfile.dev`,
/// `Dockerfile.prod`, `Containerfile.web` - which multi-image layouts
/// produce in an unbounded family and hadolint lints the same way.
pub static DOCKER: LanguageSupport = LanguageSupport {
    name: "docker",
    display_name: "Docker",
    extensions: &[".dockerfile"],
    filenames: &["Dockerfile", "Containerfile"],
    filename_prefixes: &["Dockerfile", "Containerfile"],
    tools: &[&HADOLINT],
    conventions: &[
        "Unpinned base image tags and package versions",
        "Secrets baked into layers via ENV or COPY",
        "Processes running as root",
        "ADD where COPY was meant",
        "Entrypoints that ignore SIGTERM",
    ],
    vendored_dirs: &[],
};

/// The family's entries in registration order. See `ALL_LANGUAGES`.
pub(crate) static FAMILY: &[&LanguageSupport] = &[&DOCKER];
