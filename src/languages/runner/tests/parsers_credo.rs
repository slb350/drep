//! Credo output parser.

use super::support::*;
use crate::analysis::findings::Severity;
use crate::languages::runner::*;

/// Verbatim from `mix credo --format json` 1.7. A double-hash raw string
/// because the fixture itself contains `"#` (a JSON string ending followed
/// by a comment marker), which would close an `r#"..."#` literal early.
fn credo_json() -> &'static str {
    r##"{"issues":[
  {"category":"design","check":"Credo.Check.Design.TagTODO","column":null,"column_end":null,"filename":"lib/demo.ex","line_no":2,"message":"Found a TODO tag in a comment: # TODO: fix this","priority":1,"scope":"Demo","trigger":"# TODO: fix this"},
  {"category":"readability","check":"Credo.Check.Readability.ModuleDoc","column":11,"column_end":15,"filename":"lib/demo.ex","line_no":1,"message":"Modules should have a @moduledoc tag.","priority":1,"scope":"Demo","trigger":"Demo"}]}"##
}

#[test]
fn credo_parser_reads_the_verified_fixture() {
    let spec = credo_like_spec();
    let findings = parse_output(&spec, credo_json(), "root").expect("credo json parses");
    assert_eq!(findings.len(), 2);
    assert_eq!(findings[0].file_path, "lib/demo.ex");
    assert_eq!(findings[0].kind, "Credo.Check.Design.TagTODO");
    assert_eq!(findings[0].line, 2);
    assert_eq!(findings[0].severity, Severity::Warning);
    assert_eq!(
        findings[0].message,
        "Found a TODO tag in a comment: # TODO: fix this"
    );
    assert_eq!(findings[1].file_path, "lib/demo.ex");
    assert_eq!(findings[1].kind, "Credo.Check.Readability.ModuleDoc");
    assert_eq!(findings[1].line, 1);
    assert_eq!(findings[1].severity, Severity::Info);
    assert_eq!(findings[1].message, "Modules should have a @moduledoc tag.");
}

/// Credo writes `null` for a check that has no column to point at, and that
/// is `None` - not `Some(0)`, which would place a caret on a character the
/// check never named.
#[test]
fn credo_parser_null_column_is_none_not_some_zero() {
    let spec = credo_like_spec();
    let findings = parse_output(&spec, credo_json(), "root").unwrap();
    assert_eq!(findings[0].column, None);
    assert_eq!(findings[1].column, Some(11));
}

/// A clean run is `{"issues":[]}`, and `mix credo` with nothing to say can
/// print nothing at all.
#[test]
fn credo_parser_empty_output_and_empty_issues_are_clean() {
    let spec = credo_like_spec();
    assert!(parse_output(&spec, "", "root").unwrap().is_empty());
    assert!(parse_output(&spec, "  \n\t ", "root").unwrap().is_empty());
    assert!(
        parse_output(&spec, r#"{"issues":[]}"#, "root")
            .unwrap()
            .is_empty()
    );
}

#[test]
fn credo_parser_errors_on_unparseable_input() {
    let spec = credo_like_spec();
    let err = parse_output(&spec, "** (Mix) nope", "root").expect_err("garbage is not a clean run");
    assert!(err.0.contains("credo"), "message was {:?}", err.0);
}

/// A payload without `issues` is not Credo's JSON - most often a Mix error
/// page - and one whose `issues` is the wrong kind is a schema change. Both
/// are errors rather than a silent clean run.
#[test]
fn credo_parser_errors_when_issues_is_missing_or_not_an_array() {
    let spec = credo_like_spec();
    let err = parse_output(&spec, r#"{"error":"boom"}"#, "root").unwrap_err();
    assert!(err.0.contains("issues"), "message was {:?}", err.0);
    let err = parse_output(&spec, r#"{"issues":{}}"#, "root").unwrap_err();
    assert!(err.0.contains("object"), "message was {:?}", err.0);
}

/// Credo has no severity field; its categories decide. `warning` is Credo's
/// correctness bucket and maps *up* to Error, readability and consistency
/// are style, and anything unrecognised stays a warning. One assertion per
/// branch so a collapsed arm fails by name.
#[test]
fn credo_category_mapping_covers_every_branch() {
    let spec = credo_like_spec();
    for (category, expected) in [
        ("warning", Severity::Error),
        ("refactor", Severity::Warning),
        ("design", Severity::Warning),
        ("readability", Severity::Info),
        ("consistency", Severity::Info),
        ("bogus", Severity::Warning),
    ] {
        let input = format!(
            r#"{{"issues":[{{"category":"{category}","check":"C","column":1,"filename":"a.ex","line_no":1,"message":"m"}}]}}"#
        );
        let findings = parse_output(&spec, &input, "root").unwrap();
        assert_eq!(findings[0].severity, expected, "category {category}");
    }
}

/// An issue with no `filename` falls back to the root name, exactly as the
/// ruff/eslint parser does; credo is the one place an empty path was a
/// rendered oddity rather than a narrowed drop, and both are wrong.
#[test]
fn credo_parser_missing_filename_falls_back_to_root_name() {
    let spec = credo_like_spec();
    let input =
        r#"{"issues":[{"category":"warning","check":"C","column":1,"line_no":1,"message":"m"}]}"#;
    let findings = parse_output(&spec, input, "fallback.ex").unwrap();
    assert_eq!(findings[0].file_path, "fallback.ex");
}

/// Issues in two files both land: a "first entry only" regression must not
/// pass.
#[test]
fn credo_parser_reads_findings_in_every_file() {
    let spec = credo_like_spec();
    let input = r#"{"issues":[
        {"category":"warning","check":"C1","column":1,"filename":"lib/a.ex","line_no":1,"message":"m1"},
        {"category":"design","check":"C2","column":2,"filename":"lib/b.ex","line_no":2,"message":"m2"}
    ]}"#;
    let findings = parse_output(&spec, input, "root").unwrap();
    assert_eq!(findings.len(), 2);
    assert_eq!(findings[0].file_path, "lib/a.ex");
    assert_eq!(findings[1].file_path, "lib/b.ex");
    assert_eq!(findings[1].line, 2);
}
