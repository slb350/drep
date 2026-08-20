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

use crate::cli::doctor::{DoctorArgs, missing_tools_line, run_to};
use crate::languages::spec::{LanguageSupport, ToolSpec};

fn args(path: &std::path::Path) -> DoctorArgs {
    DoctorArgs {
        path: path.to_path_buf(),
        config: None,
    }
}

#[test]
fn skipped_tool_is_not_a_problem() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("a.py"), "x = 1\n").expect("a.py");
    // No pyproject.toml → ruff is Skipped.

    let mut out = Vec::new();
    let exit = run_to(&mut out, &args(dir.path())).expect("run_to");
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

#[test]
fn missing_tools_line_renders_the_expected_sentence() {
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

#[test]
fn empty_missing_tools_line_is_none() {
    assert!(missing_tools_line(&[]).is_none());
}

/// A configured tool reports one of exactly two ways, and the trailing
/// missing-block agrees with whichever it was.
///
/// `tool_status` resolves against the real `PATH` and has no injection seam,
/// so whether `tsc` is present is a property of the machine. The previous
/// version of this test tried to cope with
/// `assert!(!ready || line.contains("configured"))` - which reads as "if
/// ready, then it says configured", something a `tsc: ready` line can never
/// satisfy. It passed only because `tsc` is absent here, and failed outright
/// on any machine that had it.
///
/// Both branches assert a real property instead, so the test means something
/// either way: available means no missing block, unavailable means one naming
/// tsc. The *rendering* of that block is pinned deterministically by
/// `missing_tools_line_renders_the_expected_sentence` above, which needs no
/// tools at all.
#[test]
fn a_configured_tool_is_either_ready_or_named_in_the_missing_block() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("tsconfig.json"), "{}").expect("tsconfig");
    std::fs::write(dir.path().join("a.ts"), "export const x = 1;\n").expect("a.ts");

    let mut out = Vec::new();
    let exit = run_to(&mut out, &args(dir.path())).expect("run_to");
    assert_eq!(exit, crate::Exit::Clean);
    let rendered = String::from_utf8(out).expect("utf8");

    let tsc_line = rendered
        .lines()
        .find(|line| line.trim_start().starts_with("tsc:"))
        .expect("a configured tool must get a line")
        .to_owned();

    if tsc_line.contains("ready") {
        assert!(
            !rendered.contains("configured tool(s) are missing"),
            "tsc resolved, so nothing is missing; rendered:\n{rendered}"
        );
    } else {
        assert!(
            tsc_line.contains("configured but not found"),
            "the only other outcome for a configured tool; got {tsc_line:?}"
        );
        assert!(
            rendered.contains("1 configured tool(s) are missing: tsc"),
            "rendered:\n{rendered}"
        );
        assert!(rendered.contains("drep exits 2"), "rendered:\n{rendered}");
    }
}

/// A tool shared by two languages is reported once.
///
/// `eslint` is configured for both JavaScript and TypeScript, so a repo with
/// `.js` and `.ts` files and no eslint binary listed it twice: "2 configured
/// tool(s) are missing: eslint, eslint". The count overstates the problem and
/// the list reads like a bug in drep rather than a missing dependency.
///
/// Skipped when eslint happens to be installed, since the line only appears
/// for a tool that is configured and absent - and the assertion below would
/// then be vacuous rather than wrong.
#[test]
fn a_tool_shared_by_two_languages_is_named_once() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("eslint.config.js"), "export default [];\n").expect("config");
    std::fs::write(dir.path().join("a.js"), "export const x = 1;\n").expect("a.js");
    std::fs::write(dir.path().join("a.ts"), "export const y: number = 1;\n").expect("a.ts");

    let mut out = Vec::new();
    run_to(&mut out, &args(dir.path())).expect("run_to");
    let rendered = String::from_utf8(out).expect("utf8");

    let Some(line) = rendered
        .lines()
        .find(|l| l.contains("configured tool(s) are missing"))
    else {
        // eslint resolved on this machine; nothing is missing to deduplicate.
        return;
    };
    assert_eq!(
        line.matches("eslint").count(),
        1,
        "eslint belongs to two languages but is one missing tool; got: {line:?}"
    );
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
    output_format: "json",
    diagnostics_stream: "stdout",
    accepts_files: true,
};

static GHOST_LANG: LanguageSupport = LanguageSupport {
    name: "ghost",
    display_name: "Ghost",
    extensions: &[".ghost"],
    tools: &[&GHOST_TOOL],
    conventions: &[],
    vendored_dirs: &[],
};

/// A second language sharing the same tool, for the deduplication half.
static SPECTRE_LANG: LanguageSupport = LanguageSupport {
    name: "spectre",
    display_name: "Spectre",
    extensions: &[".spectre"],
    tools: &[&GHOST_TOOL],
    conventions: &[],
    vendored_dirs: &[],
};

#[test]
fn a_configured_tool_that_cannot_resolve_is_returned_as_missing() {
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
#[test]
fn a_tool_shared_by_two_languages_is_returned_once() {
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
