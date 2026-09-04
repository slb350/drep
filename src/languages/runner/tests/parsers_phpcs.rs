//! PHP_CodeSniffer output parser.

use super::support::*;
use crate::analysis::findings::Severity;
use crate::languages::runner::*;

/// Verbatim from `phpcs --report=json` 3.7.2: `files` is an OBJECT keyed by
/// path, and the escaped `\/` separators are how phpcs writes them.
fn phpcs_json() -> &'static str {
    r#"{"totals":{"errors":3,"warnings":0,"fixable":3},
 "files":{"\/w\/Sample.php":{"errors":3,"warnings":0,"messages":[
    {"message":"Opening brace should be on a new line","source":"Squiz.Functions.MultiLineFunctionDeclaration.BraceOnSameLine","severity":5,"fixable":true,"type":"ERROR","line":2,"column":18},
    {"message":"TRUE, FALSE and NULL must be lowercase; expected \"null\" but found \"NULL\"","source":"Generic.PHP.LowerCaseConstant.Found","severity":5,"fixable":true,"type":"ERROR","line":4,"column":15}]}}}"#
}

#[test]
fn phpcs_parser_reads_the_verified_fixture() {
    let spec = phpcs_like_spec();
    let findings = parse_output(&spec, phpcs_json(), "root").expect("phpcs json parses");
    assert_eq!(findings.len(), 2);
    assert_eq!(
        findings[0].kind,
        "Squiz.Functions.MultiLineFunctionDeclaration.BraceOnSameLine"
    );
    assert_eq!(findings[0].line, 2);
    assert_eq!(findings[0].column, Some(18));
    assert_eq!(findings[0].message, "Opening brace should be on a new line");
    assert_eq!(findings[0].severity, Severity::Error);
    assert_eq!(findings[1].kind, "Generic.PHP.LowerCaseConstant.Found");
    assert_eq!(findings[1].line, 4);
    assert_eq!(findings[1].column, Some(15));
    assert_eq!(
        findings[1].message,
        "TRUE, FALSE and NULL must be lowercase; expected \"null\" but found \"NULL\""
    );
}

/// The `files` keys are ABSOLUTE in real output and are passed through
/// unchanged: `check` resolves absolute reported paths itself, and
/// re-relativising here would assume a cwd the parser does not know.
#[test]
fn phpcs_parser_passes_the_absolute_path_key_through_unchanged() {
    let spec = phpcs_like_spec();
    let findings = parse_output(&spec, phpcs_json(), "root").unwrap();
    assert_eq!(findings[0].file_path, "/w/Sample.php");
    assert_eq!(findings[1].file_path, "/w/Sample.php");
}

/// A clean run is `{"files":{}}` and a quiet stdout is nothing at all.
#[test]
fn phpcs_parser_empty_output_and_empty_files_are_clean() {
    let spec = phpcs_like_spec();
    assert!(parse_output(&spec, "", "root").unwrap().is_empty());
    assert!(parse_output(&spec, "  \n\t ", "root").unwrap().is_empty());
    assert!(
        parse_output(&spec, r#"{"files":{}}"#, "root")
            .unwrap()
            .is_empty()
    );
}

#[test]
fn phpcs_parser_errors_on_unparseable_input() {
    let spec = phpcs_like_spec();
    let err = parse_output(&spec, "<xml? no", "root").expect_err("garbage is not a clean run");
    assert!(err.0.contains("phpcs"), "message was {:?}", err.0);
}

/// A payload without `files` is not phpcs's JSON; one whose `files` is an
/// array belongs to a different tool's shape. Both are errors.
#[test]
fn phpcs_parser_errors_when_files_is_missing_or_not_an_object() {
    let spec = phpcs_like_spec();
    let err = parse_output(&spec, r#"{"totals":{}}"#, "root").unwrap_err();
    assert!(err.0.contains("files"), "message was {:?}", err.0);
    let err = parse_output(&spec, r#"{"files":[]}"#, "root").unwrap_err();
    assert!(err.0.contains("array"), "message was {:?}", err.0);
}

/// `type` is compared case-insensitively - phpcs has emitted `ERROR` and
/// `error` across releases - and the numeric `severity` field (a phpcs
/// reporting priority, not a level) is never read: a `severity: 1` ERROR
/// must still gate as an error.
#[test]
fn phpcs_severity_mapping_covers_every_branch() {
    let spec = phpcs_like_spec();
    for (kind, expected) in [
        ("ERROR", Severity::Error),
        ("error", Severity::Error),
        ("WARNING", Severity::Warning),
        ("notice", Severity::Warning),
    ] {
        let input = format!(
            r#"{{"files":{{"a.php":{{"messages":[{{"message":"m","source":"S","severity":1,"type":"{kind}","line":1,"column":1}}]}}}}}}"#
        );
        let findings = parse_output(&spec, &input, "root").unwrap();
        assert_eq!(findings[0].severity, expected, "type {kind}");
    }
}

/// Messages in two files both land: a "first entry only" regression must
/// not pass.
#[test]
fn phpcs_parser_reads_findings_in_every_file() {
    let spec = phpcs_like_spec();
    let input = r#"{"totals":{"errors":2},"files":{
        "/w/a.php":{"errors":1,"warnings":0,"messages":[{"message":"m1","source":"S1","severity":5,"fixable":false,"type":"ERROR","line":1,"column":1}]},
        "/w/b.php":{"errors":1,"warnings":0,"messages":[{"message":"m2","source":"S2","severity":5,"fixable":false,"type":"ERROR","line":2,"column":2}]}
    }}"#;
    let findings = parse_output(&spec, input, "root").unwrap();
    assert_eq!(findings.len(), 2);
    let mut paths: Vec<&str> = findings.iter().map(|f| f.file_path.as_str()).collect();
    paths.sort_unstable();
    assert_eq!(paths, vec!["/w/a.php", "/w/b.php"]);
}
