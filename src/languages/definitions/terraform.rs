//! Terraform: tflint over `.tf` and `.tfvars`.

use crate::languages::spec::{DEFAULT_TOOL_TIMEOUT_SECS, LanguageSupport, ToolSpec};

/// Terraform deterministic checker.
///
/// tflint's SARIF `uri` is repo-relative, verified against 0.64.0. Some of
/// its results carry a `physicalLocation` with no `region`; the SARIF
/// parser already defaults those to line 1.
pub static TFLINT: ToolSpec = ToolSpec {
    name: "tflint",
    command: &["tflint", "--format", "sarif"],
    local_paths: &[],
    config_files: &[".tflint.hcl"],
    config_flag: None,
    output_format: "sarif",
    diagnostics_stream: "stdout",
    timeout_secs: DEFAULT_TOOL_TIMEOUT_SECS,
    timeout_context: None,
    establishes_compilation: false,
    serial_in_repository: false,
    accepts_files: true,
};

/// Terraform language entry.
pub static TERRAFORM: LanguageSupport = LanguageSupport {
    name: "terraform",
    display_name: "Terraform",
    extensions: &[".tf", ".tfvars"],
    filenames: &[],
    tools: &[&TFLINT],
    conventions: &[
        "Unpinned provider versions and module sources",
        "Changes that force replacement of stateful resources",
        "Hardcoded values that belong in variables",
        "Missing tags and attributes the account policy requires",
        "count and for_each churn that recreates identical resources",
    ],
    // `.terraform` holds downloaded providers and module copies: thousands of
    // generated files per lock file revision.
    vendored_dirs: &[".terraform"],
};
