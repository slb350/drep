//! Docker: hadolint over `Dockerfile` and `Containerfile`.

use crate::languages::spec::{DEFAULT_TOOL_TIMEOUT_SECS, LanguageSupport, ToolSpec};

/// Docker deterministic checker.
///
/// hadolint's SARIF `uri` is repo-relative, verified against 2.15.1.
pub static HADOLINT: ToolSpec = ToolSpec {
    name: "hadolint",
    command: &["hadolint", "--format", "sarif"],
    local_paths: &[],
    config_files: &[".hadolint.yaml", ".hadolint.yml"],
    config_flag: None,
    output_format: "sarif",
    diagnostics_stream: "stdout",
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
/// as one.
pub static DOCKER: LanguageSupport = LanguageSupport {
    name: "docker",
    display_name: "Docker",
    extensions: &[".dockerfile"],
    filenames: &["Dockerfile", "Containerfile"],
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
