//! `cargo` (clippy JSON) output parser.

use super::support::*;
use crate::analysis::findings::Severity;
use crate::languages::runner::*;

#[test]
fn cargo_parser_keeps_only_compiler_message_events() {
    let spec = clippy_like_spec();
    let diagnostic = r#"{"reason":"compiler-message","message":{"message":"oops","code":{"code":"clippy::needless_return"},"spans":[{"file_name":"src/lib.rs","line_start":5,"column_start":9,"is_primary":true}]}}"#;
    let input = format!(
        "{diagnostic}\n{}\n{}\n",
        r#"{"reason":"build-started","message":{}}"#, r#"{"reason":"build-finished","message":{}}"#,
    );
    let findings = parse_output(&spec, &input, "root").unwrap();
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].kind, "clippy::needless_return");
    assert_eq!(findings[0].file_path, "src/lib.rs");
    assert_eq!(findings[0].line, 5);
    assert_eq!(findings[0].column, Some(9));
}

#[test]
fn cargo_parser_uses_primary_span_not_first_when_first_is_secondary() {
    let spec = clippy_like_spec();
    let input = r#"{"reason":"compiler-message","message":{"message":"x","code":{"code":"clippy::x"},"spans":[{"file_name":"note.rs","line_start":1,"column_start":1,"is_primary":false},{"file_name":"src/lib.rs","line_start":42,"column_start":7,"is_primary":true}]}}"#;
    let findings = parse_output(&spec, input, "root").unwrap();
    assert_eq!(findings[0].file_path, "src/lib.rs");
    assert_eq!(findings[0].line, 42);
    assert_eq!(findings[0].column, Some(7));
}

#[test]
fn cargo_parser_non_json_line_is_error_not_skip() {
    let spec = clippy_like_spec();
    let input = "this is not json\n";
    let err = parse_output(&spec, input, "root").unwrap_err();
    assert!(err.0.contains("non-JSON"));
}

#[test]
fn cargo_parser_falls_back_to_tool_name_when_code_absent() {
    let spec = clippy_like_spec();
    let input = r#"{"reason":"compiler-message","message":{"message":"x","spans":[{"file_name":"src/lib.rs","line_start":1,"column_start":1,"is_primary":true}]}}"#;
    let findings = parse_output(&spec, input, "root").unwrap();
    assert_eq!(findings[0].kind, "clippy");
}

/// cargo's `level` decides the displayed severity.
///
/// It was hardcoded to Error, so a clippy warning rendered as an error. The
/// gate is unaffected either way - any tool finding blocks - but the line the
/// user reads should say what the compiler said.
#[test]
fn cargo_parser_reads_the_diagnostic_level() {
    let spec = ToolSpec {
        name: "clippy",
        output_format: OutputFormat::Cargo,
        ..ToolSpec::default()
    };
    let event = |level: &str| {
        format!(
            r#"{{"reason":"compiler-message","message":{{"level":"{level}","message":"m","code":{{"code":"C1"}},"spans":[{{"is_primary":true,"file_name":"a.rs","line_start":1,"column_start":1}}]}}}}"#
        )
    };
    for (level, expected) in [
        ("error", Severity::Error),
        ("warning", Severity::Warning),
        ("note", Severity::Info),
        ("help", Severity::Info),
        ("failure-note", Severity::Error),
        ("something-new", Severity::Error),
    ] {
        let findings = parse_output(&spec, &event(level), "root").expect("cargo parse");
        assert_eq!(findings[0].severity, expected, "level {level}");
    }
}

/// Diagnostics in two files both land: a "first entry only" regression must
/// not pass.
#[test]
fn cargo_parser_reads_findings_in_every_file() {
    let spec = clippy_like_spec();
    let event = |file: &str, line: u32| {
        format!(
            r#"{{"reason":"compiler-message","message":{{"level":"warning","message":"m","code":{{"code":"C"}},"spans":[{{"is_primary":true,"file_name":"{file}","line_start":{line},"column_start":1}}]}}}}"#
        )
    };
    let input = format!("{}\n{}\n", event("src/a.rs", 1), event("src/b.rs", 2));
    let findings = parse_output(&spec, &input, "root").unwrap();
    assert_eq!(findings.len(), 2);
    assert_eq!(findings[0].file_path, "src/a.rs");
    assert_eq!(findings[1].file_path, "src/b.rs");
    assert_eq!(findings[1].line, 2);
}
