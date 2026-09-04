//! Terraform: tflint over `.tf` and `.tfvars`.

use crate::languages::spec::{
    DEFAULT_TOOL_TIMEOUT_SECS, DiagnosticsStream, LanguageSupport, OutputFormat, ToolSpec,
};

/// Terraform deterministic checker.
///
/// tflint's SARIF `uri` is repo-relative, verified against its real output.
/// Some of its results carry a `physicalLocation` with no `region`; the
/// SARIF parser already defaults those to line 1.
///
/// `--recursive` is what makes nested modules visible at all: bare tflint
/// lints only the module in its cwd, so a commit touching `modules/*/`
/// produced no findings and passed silently. Verified against the real
/// binary: the recursive run descends, emits each finding's uri relative to
/// the invocation directory, and exits 2 when any module has findings. Each
/// module's config is its own - a nested module without a `.tflint.hcl` is
/// linted under tflint's defaults rather than the root config, which is
/// tflint's documented per-module resolution, not something drep can
/// override from here.
pub static TFLINT: ToolSpec = ToolSpec {
    name: "tflint",
    command: &["tflint", "--format", "sarif", "--recursive"],
    local_paths: &[],
    config_files: &[".tflint.hcl"],
    config_flag: None,
    output_format: OutputFormat::Sarif,
    diagnostics_stream: DiagnosticsStream::Stdout,
    timeout_secs: DEFAULT_TOOL_TIMEOUT_SECS,
    timeout_context: None,
    establishes_compilation: false,
    serial_in_repository: false,
    // tflint dropped positional file arguments in v0.47: handed `main.tf` it
    // exits 1 with a SARIF `tflint-errors` run saying "Command line arguments
    // support was dropped in v0.47. Use --chdir or --filter instead.". That
    // result carries no location, and a locationless SARIF result is reported
    // as the tool failing rather than as a finding. Run the configured module
    // bare and narrow findings back to the requested files, exactly as tsc
    // and clippy do.
    accepts_files: false,
};

/// Terraform language entry.
pub static TERRAFORM: LanguageSupport = LanguageSupport {
    name: "terraform",
    display_name: "Terraform",
    extensions: &[".tf", ".tfvars"],
    filenames: &[],
    filename_prefixes: &[],
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

/// The family's entries in registration order. See `ALL_LANGUAGES`.
pub(crate) static FAMILY: &[&LanguageSupport] = &[&TERRAFORM];
