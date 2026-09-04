//! `ktlint` JSON output parser.

use super::support::*;
use crate::analysis::findings::Severity;
use crate::languages::runner::*;

/// Verbatim from `ktlint --log-level=none --reporter=json`.
fn ktlint_json() -> &'static str {
    r#"[
        {
            "file": "/private/tmp/jvmfix/Sample.kt",
            "errors": [
                { "line": 5, "column": 10, "message": "Unexpected whitespace",
                  "rule": "standard:parameter-list-spacing" },
                { "line": 6, "column": 10, "message": "Missing spacing around \"=\"",
                  "rule": "standard:op-spacing" }
            ]
        }
    ]"#
}

#[test]
fn ktlint_parser_reads_every_error_under_its_file() {
    let spec = ktlint_like_spec();
    let findings = parse_output(&spec, ktlint_json(), "root").expect("ktlint json parses");
    assert_eq!(findings.len(), 2);
    for finding in &findings {
        assert_eq!(finding.file_path, "/private/tmp/jvmfix/Sample.kt");
        assert_eq!(finding.severity, Severity::Error);
    }
    assert_eq!(findings[0].kind, "standard:parameter-list-spacing");
    assert_eq!(findings[0].line, 5);
    assert_eq!(findings[0].column, Some(10));
    assert_eq!(findings[1].message, "Missing spacing around \"=\"");
}

/// ktlint prints `[]` for a clean run, and nothing at all when it has no files
/// to look at.
#[test]
fn ktlint_parser_treats_empty_and_empty_array_as_clean() {
    let spec = ktlint_like_spec();
    assert!(parse_output(&spec, "", "root").unwrap().is_empty());
    assert!(parse_output(&spec, "[]", "root").unwrap().is_empty());
}

#[test]
fn ktlint_parser_errors_on_unparseable_input() {
    let spec = ktlint_like_spec();
    let err = parse_output(&spec, "{oops", "root").expect_err("garbage is not a clean run");
    assert!(err.0.contains("ktlint"), "message was {:?}", err.0);
}

/// A file record with no `file` falls back to the root name, exactly as the
/// ruff/eslint parser does.
#[test]
fn ktlint_parser_missing_file_falls_back_to_root_name() {
    let spec = ktlint_like_spec();
    let input = r#"[{"errors":[{"line":1,"column":1,"message":"m","rule":"r"}]}]"#;
    let findings = parse_output(&spec, input, "fallback.kt").unwrap();
    assert_eq!(findings[0].file_path, "fallback.kt");
}

/// Errors in two files both land: a "first entry only" regression must not
/// pass.
#[test]
fn ktlint_parser_reads_findings_in_every_file() {
    let spec = ktlint_like_spec();
    let input = r#"[
        {"file":"a.kt","errors":[{"line":1,"column":1,"message":"m1","rule":"r1"}]},
        {"file":"b.kt","errors":[{"line":2,"column":2,"message":"m2","rule":"r2"}]}
    ]"#;
    let findings = parse_output(&spec, input, "root").unwrap();
    assert_eq!(findings.len(), 2);
    assert_eq!(findings[0].file_path, "a.kt");
    assert_eq!(findings[1].file_path, "b.kt");
    assert_eq!(findings[1].line, 2);
}
