//! RuboCop output parser.

use super::support::*;
use crate::analysis::findings::Severity;
use crate::languages::runner::*;

/// Verbatim from `rubocop --format json` 1.90.0 (metadata and summary kept,
/// offenses complete).
fn rubocop_json() -> &'static str {
    r#"{"metadata":{"rubocop_version":"1.90.0"},
 "files":[{"path":"Sample.rb","offenses":[
   {"severity":"convention","message":"Space inside parentheses detected.","cop_name":"Layout/SpaceInsideParens","corrected":false,"correctable":true,
    "location":{"start_line":1,"start_column":9,"last_line":1,"last_column":9,"length":1,"line":1,"column":9}},
   {"severity":"warning","message":"Useless assignment to variable - `y`.","cop_name":"Lint/UselessAssignment","corrected":false,"correctable":true,
    "location":{"start_line":2,"start_column":3,"last_line":2,"last_column":3,"length":1,"line":2,"column":3}}]}],
 "summary":{"offense_count":2,"target_file_count":1,"inspected_file_count":1}}"#
}

#[test]
fn rubocop_parser_reads_the_verified_fixture() {
    let spec = rubocop_like_spec();
    let findings = parse_output(&spec, rubocop_json(), "root").expect("rubocop json parses");
    assert_eq!(findings.len(), 2);
    assert_eq!(findings[0].file_path, "Sample.rb");
    assert_eq!(findings[0].kind, "Layout/SpaceInsideParens");
    assert_eq!(findings[0].line, 1);
    assert_eq!(findings[0].column, Some(9));
    assert_eq!(findings[0].message, "Space inside parentheses detected.");
    assert_eq!(findings[0].severity, Severity::Info);
    assert_eq!(findings[1].file_path, "Sample.rb");
    assert_eq!(findings[1].kind, "Lint/UselessAssignment");
    assert_eq!(findings[1].line, 2);
    assert_eq!(findings[1].column, Some(3));
    assert_eq!(findings[1].message, "Useless assignment to variable - `y`.");
    assert_eq!(findings[1].severity, Severity::Warning);
}

/// A clean run is `{"files":[]}`, and a quiet one is nothing on stdout at
/// all. Both are clean; anything else that will not parse is an error.
#[test]
fn rubocop_parser_empty_output_and_empty_files_are_clean() {
    let spec = rubocop_like_spec();
    assert!(parse_output(&spec, "", "root").unwrap().is_empty());
    assert!(parse_output(&spec, "  \n\t ", "root").unwrap().is_empty());
    assert!(
        parse_output(&spec, r#"{"files":[]}"#, "root")
            .unwrap()
            .is_empty()
    );
}

#[test]
fn rubocop_parser_errors_on_unparseable_input() {
    let spec = rubocop_like_spec();
    let err = parse_output(&spec, "{oops", "root").expect_err("garbage is not a clean run");
    assert!(err.0.contains("rubocop"), "message was {:?}", err.0);
}

/// A payload without `files` is not RuboCop's JSON; one whose `files` is the
/// wrong kind is a schema change. Both are errors, each naming what arrived.
#[test]
fn rubocop_parser_errors_when_files_is_missing_or_not_an_array() {
    let spec = rubocop_like_spec();
    let err = parse_output(&spec, r#"{"metadata":{}}"#, "root").unwrap_err();
    assert!(err.0.contains("files"), "message was {:?}", err.0);
    let err = parse_output(&spec, r#"{"files":"nope"}"#, "root").unwrap_err();
    assert!(err.0.contains("string"), "message was {:?}", err.0);
}

/// Every severity branch, one assertion per branch, including the fallback
/// for an unrecognised value.
#[test]
fn rubocop_severity_mapping_covers_every_branch() {
    let spec = rubocop_like_spec();
    for (severity, expected) in [
        ("fatal", Severity::Error),
        ("error", Severity::Error),
        ("warning", Severity::Warning),
        ("convention", Severity::Info),
        ("refactor", Severity::Info),
        ("info", Severity::Info),
        ("bogus", Severity::Warning),
    ] {
        let input = format!(
            r#"{{"files":[{{"path":"a.rb","offenses":[{{"severity":"{severity}","cop_name":"C","message":"m","location":{{"line":1,"column":2}}}}]}}]}}"#
        );
        let findings = parse_output(&spec, &input, "root").unwrap();
        assert_eq!(findings[0].severity, expected, "severity {severity}");
    }
}

/// Current RuboCop writes both `start_line`/`start_column` and
/// `line`/`column`; older releases wrote only the latter. The parser prefers
/// the documented key but must still read a location that carries the legacy
/// spelling alone, or every file on an older RuboCop would report line 1.
#[test]
fn rubocop_parser_falls_back_to_legacy_line_and_column_keys() {
    let spec = rubocop_like_spec();
    let input = r#"{"files":[{"path":"old.rb","offenses":[
        {"severity":"warning","message":"m","cop_name":"C",
         "location":{"line":7,"column":3}}]}]}"#;
    let findings = parse_output(&spec, input, "root").unwrap();
    assert_eq!(findings[0].line, 7);
    assert_eq!(findings[0].column, Some(3));
}

/// `start_line` wins when both spellings are present and disagree: it is the
/// documented key, and treating the pair as interchangeable would let a
/// future schema quietly change which line a finding points at.
#[test]
fn rubocop_parser_prefers_start_line_when_both_keys_are_present() {
    let spec = rubocop_like_spec();
    let input = r#"{"files":[{"path":"a.rb","offenses":[
        {"severity":"warning","message":"m","cop_name":"C",
         "location":{"start_line":10,"start_column":4,"line":99,"column":99}}]}]}"#;
    let findings = parse_output(&spec, input, "root").unwrap();
    assert_eq!(findings[0].line, 10);
    assert_eq!(findings[0].column, Some(4));
}

/// A file entry with no `path` falls back to the root name, exactly as the
/// ruff/eslint parser does.
#[test]
fn rubocop_parser_missing_path_falls_back_to_root_name() {
    let spec = rubocop_like_spec();
    let input = r#"{"files":[{"offenses":[{"severity":"warning","message":"m","cop_name":"C","location":{"line":1,"column":2}}]}]}"#;
    let findings = parse_output(&spec, input, "fallback.rb").unwrap();
    assert_eq!(findings[0].file_path, "fallback.rb");
}

/// Offenses in two files both land: a "first entry only" regression must
/// not pass.
#[test]
fn rubocop_parser_reads_findings_in_every_file() {
    let spec = rubocop_like_spec();
    let input = r#"{"files":[
        {"path":"a.rb","offenses":[{"severity":"warning","message":"m1","cop_name":"C1","location":{"line":1,"column":1}}]},
        {"path":"b.rb","offenses":[{"severity":"error","message":"m2","cop_name":"C2","location":{"line":2,"column":2}}]}
    ]}"#;
    let findings = parse_output(&spec, input, "root").unwrap();
    assert_eq!(findings.len(), 2);
    assert_eq!(findings[0].file_path, "a.rb");
    assert_eq!(findings[1].file_path, "b.rb");
    assert_eq!(findings[1].line, 2);
}
