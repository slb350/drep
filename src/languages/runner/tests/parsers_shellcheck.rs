//! ShellCheck output parser.
//!
//! Wired in via `#[cfg(test)] mod tests;` in the parent module. These files
//! were orphaned once - present on disk but reachable by no `mod`
//! declaration, so cargo never compiled them and appending invalid Rust did
//! not fail the build. If you add a file here, declare it in this
//! directory's `mod.rs`.

use super::support::*;
use crate::analysis::findings::Severity;
use crate::languages::runner::*;

/// Verbatim from `shellcheck -f json` on a four-line script.
fn shellcheck_json() -> &'static str {
    r#"[{"file":"t.sh","line":2,"endLine":2,"column":1,"endColumn":4,"level":"warning","code":2034,"message":"foo appears unused. Verify use (or export if used externally).","fix":null},
 {"file":"t.sh","line":3,"endLine":3,"column":6,"endColumn":10,"level":"info","code":2086,"message":"Double quote to prevent globbing and word splitting.","fix":{"replacements":[]}}]"#
}

/// The `code` field is a number on the wire, but every place a user meets a
/// ShellCheck rule - its wiki, `# shellcheck disable=` directives, CI logs -
/// names it with the SC prefix. A bare `2034` in `kind` is a rule the user
/// cannot grep for.
#[test]
fn shellcheck_parser_prefixes_the_numeric_code_with_sc() {
    let spec = shellcheck_like_spec();
    let findings = parse_output(&spec, shellcheck_json(), "root").expect("shellcheck json parses");
    assert_eq!(findings.len(), 2);
    assert_eq!(findings[0].kind, "SC2034");
    assert_eq!(findings[1].kind, "SC2086");
}

/// The verified fixture, field by field: relative file path, true line and
/// column, and the message as the tool wrote it.
#[test]
fn shellcheck_parser_reads_the_verified_fixture() {
    let spec = shellcheck_like_spec();
    let findings = parse_output(&spec, shellcheck_json(), "root").unwrap();
    assert_eq!(findings[0].file_path, "t.sh");
    assert_eq!(findings[0].line, 2);
    assert_eq!(findings[0].column, Some(1));
    assert_eq!(
        findings[0].message,
        "foo appears unused. Verify use (or export if used externally)."
    );
    assert_eq!(findings[1].line, 3);
    assert_eq!(findings[1].column, Some(6));
    assert_eq!(
        findings[1].message,
        "Double quote to prevent globbing and word splitting."
    );
}

/// A clean shellcheck run prints nothing at all. Whitespace-only output is
/// the same case, and guessing "clean" on unparseable output is what this
/// parser exists to refuse.
#[test]
fn shellcheck_parser_empty_and_whitespace_output_are_clean() {
    let spec = shellcheck_like_spec();
    assert!(parse_output(&spec, "", "root").unwrap().is_empty());
    assert!(parse_output(&spec, "  \n\t ", "root").unwrap().is_empty());
}

/// Unparseable input must be an error naming the tool: swallowing it would
/// report the file clean when we do not know what the tool said.
#[test]
fn shellcheck_parser_errors_on_unparseable_input() {
    let spec = shellcheck_like_spec();
    let err = parse_output(&spec, "not json at all", "root").expect_err("not a clean run");
    assert!(err.0.contains("shellcheck"), "message was {:?}", err.0);
}

/// A non-array payload names the kind it received, so a tool printing an
/// error object instead of a diagnostics array is distinguishable from a
/// broken pipe.
#[test]
fn shellcheck_parser_rejects_object_payload_naming_the_kind() {
    let spec = shellcheck_like_spec();
    let err = parse_output(&spec, r#"{"error":"nope"}"#, "root").unwrap_err();
    assert!(err.0.contains("object"), "message was {:?}", err.0);
}

/// Every `level` branch, one assertion per branch, including the fallback
/// for an unrecognised value - without the last one a future ShellCheck
/// level name would silently collapse into whichever arm the compiler left.
#[test]
fn shellcheck_level_mapping_covers_every_branch() {
    let spec = shellcheck_like_spec();
    for (level, expected) in [
        ("error", Severity::Error),
        ("warning", Severity::Warning),
        ("info", Severity::Info),
        ("style", Severity::Info),
        ("bogus", Severity::Warning),
    ] {
        let input = format!(
            r#"[{{"file":"t.sh","line":1,"column":1,"level":"{level}","code":1,"message":"m"}}]"#
        );
        let findings = parse_output(&spec, &input, "root").unwrap();
        assert_eq!(findings[0].severity, expected, "level {level}");
    }
}
