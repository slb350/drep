//! `json` (ruff/eslint) output parser.

use super::support::*;
use crate::analysis::findings::Severity;
use crate::languages::runner::*;

#[test]
fn json_parser_empty_string_is_zero_findings_not_error() {
    let spec = ruff_like_spec();
    let findings = parse_output(&spec, "", "root").expect("empty is []");
    assert!(findings.is_empty());
}

#[test]
fn json_parser_ruff_shape() {
    let spec = ruff_like_spec();
    let input = r#"[{"code":"F401","filename":"a.py","location":{"row":3,"column":5},"message":"unused","fix":{"message":"remove"}}]"#;
    let findings = parse_output(&spec, input, "root").unwrap();
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].kind, "F401");
    assert_eq!(findings[0].file_path, "a.py");
    assert_eq!(findings[0].line, 3);
    assert_eq!(findings[0].column, Some(5));
    assert_eq!(findings[0].message, "unused");
    assert_eq!(findings[0].suggestion.as_deref(), Some("remove"));
}

#[test]
fn json_parser_eslint_shape_two_messages_become_two_findings() {
    let spec = eslint_like_spec();
    let input = r#"[{
        "filePath": "app.js",
        "messages": [
            {"ruleId": "no-unused", "line": 7, "column": 3, "message": "x is unused"},
            {"ruleId": "no-shadow", "line": 11, "column": 5, "message": "y shadows"}
        ]
    }]"#;
    let findings = parse_output(&spec, input, "root").unwrap();
    assert_eq!(findings.len(), 2);
    assert_eq!(findings[0].kind, "no-unused");
    assert_eq!(findings[0].file_path, "app.js");
    assert_eq!(findings[1].kind, "no-shadow");
    assert_eq!(findings[1].file_path, "app.js");
}

#[test]
fn json_parser_rejects_object_payload() {
    let spec = ruff_like_spec();
    let err = parse_output(&spec, r#"{"not":"an array"}"#, "root").unwrap_err();
    assert!(err.0.contains("array"));
}

/// The error names the kind it actually got, not only the kind it wanted.
///
/// Asserting on "array" alone passes however `json_kind_name` answers, so the
/// half of the message that tells you *what arrived* went unpinned - which is
/// the half that distinguishes a tool printing a bare number from one printing
/// a wrapped object.
#[test]
fn json_parser_error_names_the_kind_it_received() {
    let spec = ruff_like_spec();
    for (payload, kind) in [
        (r#"{"not":"an array"}"#, "object"),
        ("12", "number"),
        (r#""text""#, "string"),
        ("true", "bool"),
        ("null", "null"),
    ] {
        let err = parse_output(&spec, payload, "root").unwrap_err();
        assert!(
            err.0.contains(kind),
            "{payload} should be reported as {kind}, got {}",
            err.0
        );
    }
}

#[test]
fn json_parser_rejects_invalid_json() {
    let spec = ruff_like_spec();
    assert!(parse_output(&spec, "not json at all", "root").is_err());
}

#[test]
fn json_parser_missing_filename_falls_back_to_root_name() {
    let spec = ruff_like_spec();
    let input = r#"[{"code":"E101","location":{"row":2,"column":1},"message":"x"}]"#;
    let findings = parse_output(&spec, input, "fallback.py").unwrap();
    assert_eq!(findings[0].file_path, "fallback.py");
}

/// eslint's per-message `severity` is read: 2 is an error, 1 is a warning.
/// It was never read, so every eslint finding rendered as an error - the same
/// display-truth defect the tsc severity fix repaired, and just as invisible
/// to the gate, which blocks on any tool finding.
#[test]
fn eslint_parser_reads_the_per_message_severity() {
    let spec = eslint_like_spec();
    let input = r#"[{
        "filePath": "app.js",
        "messages": [
            {"ruleId": "no-unused", "severity": 2, "line": 7, "message": "x is unused"},
            {"ruleId": "no-shadow", "severity": 1, "line": 11, "message": "y shadows"},
            {"ruleId": "parse", "line": 1, "message": "no severity field"}
        ]
    }]"#;
    let findings = parse_output(&spec, input, "root").unwrap();
    assert_eq!(findings.len(), 3);
    assert_eq!(findings[0].severity, Severity::Error);
    assert_eq!(findings[1].severity, Severity::Warning);
    assert_eq!(
        findings[2].severity,
        Severity::Error,
        "an absent field keeps the historical default"
    );
}

/// ruff diagnostics in two files both land: a "first entry only" regression
/// must not pass.
#[test]
fn ruff_parser_reads_findings_in_every_file() {
    let spec = ruff_like_spec();
    let input = r#"[
        {"code":"F401","filename":"a.py","location":{"row":1,"column":1},"message":"m1"},
        {"code":"F841","filename":"b.py","location":{"row":2,"column":2},"message":"m2"}
    ]"#;
    let findings = parse_output(&spec, input, "root").unwrap();
    assert_eq!(findings.len(), 2);
    assert_eq!(findings[0].file_path, "a.py");
    assert_eq!(findings[1].file_path, "b.py");
    assert_eq!(findings[1].line, 2);
}

/// eslint reports one record per file; findings in two records both land.
#[test]
fn eslint_parser_reads_findings_in_every_file() {
    let spec = eslint_like_spec();
    let input = r#"[
        {"filePath":"a.js","messages":[{"ruleId":"r1","line":1,"column":1,"message":"m1"}]},
        {"filePath":"b.js","messages":[{"ruleId":"r2","line":2,"column":2,"message":"m2"}]}
    ]"#;
    let findings = parse_output(&spec, input, "root").unwrap();
    assert_eq!(findings.len(), 2);
    assert_eq!(findings[0].file_path, "a.js");
    assert_eq!(findings[1].file_path, "b.js");
    assert_eq!(findings[1].line, 2);
}
