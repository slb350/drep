//! sqlfluff output parser.
//!
//! Wired in via `#[cfg(test)] mod tests;` in the parent module. These files
//! were orphaned once - present on disk but reachable by no `mod`
//! declaration, so cargo never compiled them and appending invalid Rust did
//! not fail the build. If you add a file here, declare it in this
//! directory's `mod.rs`.

use super::support::*;
use crate::analysis::findings::Severity;
use crate::languages::runner::*;

/// Verbatim from `sqlfluff lint --format json` 4.3.0 (truncated to two
/// violations).
fn sqlfluff_json() -> &'static str {
    r#"[{"filepath":"migration.sql","violations":[
  {"start_line_no":2,"start_line_pos":1,"code":"LT02","description":"Expected indent of 4 spaces.","name":"layout.indent","warning":false,"fixes":[]},
  {"start_line_no":3,"start_line_pos":1,"code":"CP02","description":"Unquoted identifiers must be consistently lower case.","name":"capitalisation.identifiers","warning":false,"fixes":[]}]}]"#
}

#[test]
fn sqlfluff_parser_reads_the_verified_fixture() {
    let spec = sqlfluff_like_spec();
    let findings = parse_output(&spec, sqlfluff_json(), "root").expect("sqlfluff json parses");
    assert_eq!(findings.len(), 2);
    assert_eq!(findings[0].file_path, "migration.sql");
    assert_eq!(findings[0].kind, "LT02");
    assert_eq!(findings[0].line, 2);
    assert_eq!(findings[0].column, Some(1));
    assert_eq!(findings[0].message, "Expected indent of 4 spaces.");
    assert_eq!(findings[0].severity, Severity::Error);
    assert_eq!(findings[1].file_path, "migration.sql");
    assert_eq!(findings[1].kind, "CP02");
    assert_eq!(findings[1].line, 3);
    assert_eq!(findings[1].column, Some(1));
    assert_eq!(
        findings[1].message,
        "Unquoted identifiers must be consistently lower case."
    );
    assert_eq!(findings[1].severity, Severity::Error);
}

/// A clean run is `[]`, and no matching files is nothing on stdout at all.
#[test]
fn sqlfluff_parser_empty_output_and_empty_array_are_clean() {
    let spec = sqlfluff_like_spec();
    assert!(parse_output(&spec, "", "root").unwrap().is_empty());
    assert!(parse_output(&spec, "  \n\t ", "root").unwrap().is_empty());
    assert!(parse_output(&spec, "[]", "root").unwrap().is_empty());
}

#[test]
fn sqlfluff_parser_errors_on_unparseable_input() {
    let spec = sqlfluff_like_spec();
    let err = parse_output(&spec, "=== [path] violation", "root")
        .expect_err("garbage is not a clean run");
    assert!(err.0.contains("sqlfluff"), "message was {:?}", err.0);
}

/// A non-array payload names the kind it received.
#[test]
fn sqlfluff_parser_rejects_object_payload_naming_the_kind() {
    let spec = sqlfluff_like_spec();
    let err = parse_output(&spec, r#"{"summary":{}}"#, "root").unwrap_err();
    assert!(err.0.contains("object"), "message was {:?}", err.0);
}

/// There is no severity string; `warning` is a boolean. A violation is an
/// error unless the project explicitly downgraded the rule, and an absent
/// flag means the same as `false`. One assertion per branch.
#[test]
fn sqlfluff_warning_flag_mapping_covers_every_branch() {
    let spec = sqlfluff_like_spec();
    for (warning, expected) in [
        (r#","warning":true"#, Severity::Warning),
        (r#","warning":false"#, Severity::Error),
        ("", Severity::Error),
    ] {
        let input = format!(
            r#"[{{"filepath":"a.sql","violations":[{{"start_line_no":1,"start_line_pos":1,"code":"LT01","description":"m"{warning}}}]}}]"#
        );
        let findings = parse_output(&spec, &input, "root").unwrap();
        assert_eq!(findings[0].severity, expected, "with {warning}");
    }
}
