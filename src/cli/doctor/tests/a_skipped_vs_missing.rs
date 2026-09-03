//! A3 + A4: distinguishing "Skipped" (the project's choice) from
//! "Unavailable" (a problem drep should report).
//!
//! A3: a directory with `a.py` and **no** `pyproject.toml` reports `ruff:` as
//! `not configured`. No `missing tools` block.
//!
//! A4: a directory with `a.py` and a `pyproject.toml` where ruff cannot be
//! resolved reports `ruff:` as one of the non-ready forms and the trailing
//! "configured tool(s) are missing: ruff" block appears. To keep this
//! deterministic on machines where ruff happens to be on PATH, the criterion
//! pins the rendering directly via `missing_tools_line`.

use crate::cli::doctor::{DoctorArgs, missing_tools_line};
use crate::languages::spec::{LanguageSupport, ToolSpec};
use crate::test_support::write_executable;

fn args(path: &std::path::Path) -> DoctorArgs {
    DoctorArgs {
        path: path.to_path_buf(),
        config: None,
    }
}

#[tokio::test]
async fn skipped_tool_is_not_a_problem() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("a.py"), "x = 1\n").expect("a.py");
    // No pyproject.toml → ruff is Skipped.

    let mut out = Vec::new();
    let exit = super::run_scoped(&mut out, &args(dir.path()), dir.path())
        .await
        .expect("run_to");
    assert_eq!(exit, crate::Exit::Clean);
    let rendered = String::from_utf8(out).expect("utf8");

    let ruff_line = rendered
        .lines()
        .find(|line| line.trim_start().starts_with("ruff:"))
        .expect("a ruff: line must appear under Deterministic checks");
    assert!(
        ruff_line.contains("not configured"),
        "skipped tool reports `not configured`; got: {ruff_line:?}"
    );
    assert!(
        !rendered.contains("configured tool(s) are missing"),
        "a skipped tool must not produce a missing-tools block; rendered:\n{rendered}"
    );
}

#[tokio::test]
async fn nested_workspace_configuration_is_reported_ready() {
    let dir = tempfile::tempdir().expect("tempdir");
    let bin = dir.path().join("venv/bin/ruff");
    std::fs::create_dir_all(bin.parent().unwrap()).expect("bin dir");
    write_executable(&bin, "#!/bin/sh\nexit 0\n");
    let member = dir.path().join("apps/web");
    std::fs::create_dir_all(&member).expect("member");
    std::fs::write(member.join("pyproject.toml"), "").expect("config");
    std::fs::write(member.join("a.py"), "x = 1\n").expect("source");

    let mut out = Vec::new();
    super::run_scoped(&mut out, &args(dir.path()), dir.path())
        .await
        .expect("run_to");
    let rendered = String::from_utf8(out).expect("utf8");

    assert!(
        rendered.contains("ruff: ready in 1 workspace(s)"),
        "nested configuration must be discovered: {rendered}"
    );
}

#[tokio::test]
async fn root_configuration_keeps_the_plain_ready_status() {
    let dir = tempfile::tempdir().expect("tempdir");
    let bin = dir.path().join("venv/bin/ruff");
    std::fs::create_dir_all(bin.parent().unwrap()).expect("bin dir");
    write_executable(&bin, "#!/bin/sh\nexit 0\n");
    std::fs::write(dir.path().join("pyproject.toml"), "").expect("config");
    std::fs::write(dir.path().join("a.py"), "x = 1\n").expect("source");

    let mut out = Vec::new();
    super::run_scoped(&mut out, &args(dir.path()), dir.path())
        .await
        .expect("run_to");
    let rendered = String::from_utf8(out).expect("utf8");
    let ruff_line = rendered
        .lines()
        .find(|line| line.trim_start().starts_with("ruff:"))
        .expect("ruff status");

    assert_eq!(
        ruff_line.trim(),
        "ruff: ready",
        "a single root configuration is not a nested workspace summary"
    );
}

#[tokio::test]
async fn missing_tools_line_renders_the_expected_sentence() {
    // Pinned directly: a test that exercised the live `tool_status` would be
    // non-deterministic on a developer machine that happens to have `ruff`
    // on PATH. The rendering is the contract; it must not depend on what is
    // installed.
    let line = missing_tools_line(&["ruff"]).expect("non-empty");
    assert!(
        line.contains("1 configured tool(s) are missing: ruff"),
        "got: {line:?}"
    );
    assert!(line.contains("drep exits 2"), "got: {line:?}");
}

#[tokio::test]
async fn empty_missing_tools_line_is_none() {
    assert!(missing_tools_line(&[]).is_none());
}

/// A configured tool that cannot resolve is returned as missing, on any
/// machine.
///
/// The two tests above both branch on whether a real binary happens to be
/// installed, and both take their "nothing is missing" path when it is - which
/// is the same path a `write_tools_section` that never records anything takes.
/// cargo-mutants deleted the `!` from `!missing.contains(&spec.name)`, so the
/// list could only ever stay empty, and the whole doctor suite still passed.
///
/// This asks the question with no dependency on the machine: a language whose
/// tool is a command name nothing can have, configured by a file that does
/// exist, must come back named.
static GHOST_TOOL: ToolSpec = ToolSpec {
    name: "ghost-linter",
    command: &["drep-ghost-linter-no-machine-has-this"],
    local_paths: &[],
    config_files: &["ghost.config"],
    config_flag: None,
    output_format: "json",
    diagnostics_stream: "stdout",
    timeout_secs: 120,
    timeout_context: None,
    establishes_compilation: false,
    serial_in_repository: false,
    accepts_files: true,
};

static GHOST_LANG: LanguageSupport = LanguageSupport {
    name: "ghost",
    display_name: "Ghost",
    extensions: &[".ghost"],
    filenames: &[],
    tools: &[&GHOST_TOOL],
    conventions: &[],
    vendored_dirs: &[],
};

/// A second language sharing the same tool, for the deduplication half.
static SPECTRE_LANG: LanguageSupport = LanguageSupport {
    name: "spectre",
    display_name: "Spectre",
    extensions: &[".spectre"],
    filenames: &[],
    tools: &[&GHOST_TOOL],
    conventions: &[],
    vendored_dirs: &[],
};

#[tokio::test]
async fn a_configured_tool_that_cannot_resolve_is_returned_as_missing() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("ghost.config"), "").expect("config");

    let mut out = Vec::new();
    let missing =
        crate::cli::doctor::write_tools_section(&mut out, &[(&GHOST_LANG, Vec::new())], dir.path())
            .expect("write_tools_section");

    assert_eq!(
        missing,
        ["ghost-linter"],
        "a configured tool that cannot resolve must be reported missing"
    );
}

/// The other half of the same condition: one entry, not two.
///
/// `eslint` belongs to both JavaScript and TypeScript, and a repo with both
/// and no eslint binary once reported "2 configured tool(s) are missing:
/// eslint, eslint". The test that covers this for the real eslint returns
/// early when eslint is installed, so it cannot be relied on either.
#[tokio::test]
async fn a_tool_shared_by_two_languages_is_returned_once() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("ghost.config"), "").expect("config");

    let mut out = Vec::new();
    let missing = crate::cli::doctor::write_tools_section(
        &mut out,
        &[(&GHOST_LANG, Vec::new()), (&SPECTRE_LANG, Vec::new())],
        dir.path(),
    )
    .expect("write_tools_section");

    assert_eq!(
        missing,
        ["ghost-linter"],
        "one tool shared by two languages is one missing tool"
    );
}
