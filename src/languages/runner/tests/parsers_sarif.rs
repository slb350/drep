//! SARIF 2.1.0 output parser.

use super::support::*;
use crate::analysis::findings::Severity;
use crate::languages::runner::*;

/// Trimmed from a real `checkstyle -c checkstyle.xml -f sarif` run: the driver
/// block and rule descriptions are dropped, the two results are verbatim.
fn checkstyle_sarif() -> &'static str {
    r#"{
      "$schema": "https://raw.githubusercontent.com/oasis-tcs/sarif-spec/master/Schemata/sarif-schema-2.1.0.json",
      "version": "2.1.0",
      "runs": [
        {
          "tool": { "driver": { "name": "Checkstyle", "version": "14.1.0" } },
          "results": [
            {
              "level": "error",
              "locations": [
                { "physicalLocation": {
                    "artifactLocation": { "uri": "file:/private/tmp/jvmfix/Sample.java" },
                    "region": { "startColumn": 8, "startLine": 2 } } }
              ],
              "message": { "id": "import.unused", "text": "Unused import - java.util.List." },
              "ruleId": "com.puppycrawl.tools.checkstyle.checks.imports.UnusedImportsCheck"
            },
            {
              "level": "warning",
              "locations": [
                { "physicalLocation": {
                    "artifactLocation": { "uri": "file:/private/tmp/jvmfix/Sample.java" },
                    "region": { "startColumn": 14, "startLine": 5 } } }
              ],
              "message": { "id": "ws.notFollowed", "text": "'=' is not followed by whitespace." },
              "ruleId": "com.puppycrawl.tools.checkstyle.checks.whitespace.WhitespaceAroundCheck"
            }
          ]
        }
      ]
    }"#
}

#[test]
fn sarif_parser_reads_rule_location_and_message() {
    let spec = checkstyle_like_spec();
    let findings = parse_output(&spec, checkstyle_sarif(), "root").expect("sarif parses");
    assert_eq!(findings.len(), 2);
    assert_eq!(
        findings[0].kind,
        "com.puppycrawl.tools.checkstyle.checks.imports.UnusedImportsCheck"
    );
    assert_eq!(findings[0].line, 2);
    assert_eq!(findings[0].column, Some(8));
    assert_eq!(findings[0].message, "Unused import - java.util.List.");
}

/// One SARIF result naming `uri`, for the URI-handling tests.
fn sarif_for_uri(uri: &str) -> String {
    format!(
        r#"{{ "version": "2.1.0", "runs": [ {{ "results": [
            {{ "level": "warning",
              "locations": [ {{ "physicalLocation": {{
                  "artifactLocation": {{ "uri": "{uri}" }},
                  "region": {{ "startLine": 1 }} }} }} ],
              "message": {{ "text": "m" }}, "ruleId": "r" }}
        ] }} ] }}"#
    )
}

/// SARIF carries a URI, drep matches findings against the paths it was asked
/// to check. A finding filed under `file:/private/tmp/...` is filed under a
/// path that never matches, so it is dropped as belonging to another file.
#[test]
fn sarif_parser_strips_the_file_uri_scheme() {
    let spec = checkstyle_like_spec();
    let findings = parse_output(&spec, checkstyle_sarif(), "root").unwrap();
    assert_eq!(findings[0].file_path, "/private/tmp/jvmfix/Sample.java");
}

/// checkstyle's SarifLogger percent-encodes exactly the space and the double
/// quote in a file name (`renderFileNameUri`). Left encoded, the finding's
/// path never matches the file drep was asked to check and displays mangled.
#[test]
fn sarif_parser_percent_decodes_the_uri_path() {
    let spec = checkstyle_like_spec();
    let sarif = sarif_for_uri("file:/repo/My%20Sources/Has%22Quote%22.java");
    let findings = parse_output(&spec, &sarif, "root").unwrap();
    assert_eq!(findings[0].file_path, "/repo/My Sources/Has\"Quote\".java");
}

/// The drive-root rule, end to end through the parser; `strip_drive_root` has
/// the reasoning.
#[test]
fn sarif_parser_drops_the_root_slash_before_a_windows_drive() {
    let spec = checkstyle_like_spec();
    let findings =
        parse_output(&spec, &sarif_for_uri("file:/C:/repo/Sample.java"), "root").unwrap();
    assert_eq!(findings[0].file_path, "C:/repo/Sample.java");
}

/// Every condition on the drive prefix has to hold, and each fixture here fails
/// exactly one of them: the drive strip must not fire on an ordinary POSIX
/// path, on a first component that merely starts with a letter, or on a name
/// whose colon is not a drive separator.
#[test]
fn sarif_parser_keeps_paths_that_only_look_like_a_drive() {
    let spec = checkstyle_like_spec();
    for path in [
        // No drive letter at all - the ordinary POSIX case.
        "/repo/Sample.java",
        // A letter, but no colon after it.
        "/C/repo/Sample.java",
        // A colon, but two characters in rather than one.
        "/repo:/Sample.java",
        // A drive-looking prefix that is not bounded by a separator.
        "/C:x/Sample.java",
        // Already drive-absolute: nothing to strip, and stripping would eat
        // the drive letter.
        "C:/repo/Sample.java",
    ] {
        let sarif = sarif_for_uri(&format!("file:{path}"));
        let findings = parse_output(&spec, &sarif, "root").unwrap();
        assert_eq!(findings[0].file_path, path, "{path} should pass through");
    }
}

/// A `%` that does not open a valid hex triplet is a literal, not an encoding:
/// a file genuinely named `100%.java` must survive the round trip.
#[test]
fn sarif_parser_leaves_malformed_percent_sequences_alone() {
    let spec = checkstyle_like_spec();
    for uri in [
        "file:/repo/100%.java",
        "file:/repo/100%2x.java",
        "file:/repo/ends%2.java",
    ] {
        let sarif = sarif_for_uri(uri);
        let findings = parse_output(&spec, &sarif, "root").unwrap();
        assert_eq!(
            findings[0].file_path,
            uri.strip_prefix("file:").unwrap(),
            "{uri} should pass through undecoded"
        );
    }
}

#[test]
fn sarif_parser_maps_the_sarif_level_to_severity() {
    let spec = checkstyle_like_spec();
    let findings = parse_output(&spec, checkstyle_sarif(), "root").unwrap();
    assert_eq!(findings[0].severity, Severity::Error);
    assert_eq!(findings[1].severity, Severity::Warning);
}

/// checkstyle emits `note` for its INFO severity level. `none` is SARIF for
/// "this rule was evaluated and had nothing to say". Neither is a warning, and
/// both must stay distinguishable from one: the fixture above never carries
/// either, so only a dedicated test keeps the arm from collapsing into the
/// catch-all.
#[test]
fn sarif_levels_note_and_none_map_to_info_not_warning() {
    let spec = checkstyle_like_spec();
    let sarif = r#"{
      "version": "2.1.0",
      "runs": [ { "results": [
        { "level": "note",
          "locations": [ { "physicalLocation": {
              "artifactLocation": { "uri": "file:/repo/A.java" },
              "region": { "startLine": 1 } } } ],
          "message": { "text": "informational" },
          "ruleId": "com.puppycrawl.tools.checkstyle.checks.SomeInfoCheck" },
        { "level": "none",
          "locations": [ { "physicalLocation": {
              "artifactLocation": { "uri": "file:/repo/A.java" },
              "region": { "startLine": 2 } } } ],
          "message": { "text": "evaluated, nothing to say" },
          "ruleId": "com.puppycrawl.tools.checkstyle.checks.SomeQuietCheck" }
      ] } ]
    }"#;
    let findings = parse_output(&spec, sarif, "root").unwrap();
    assert_eq!(findings.len(), 2);
    assert_eq!(
        findings[0].severity,
        Severity::Info,
        "note is informational"
    );
    assert_eq!(
        findings[1].severity,
        Severity::Info,
        "none must not become a warning"
    );
}

#[test]
fn sarif_parser_treats_no_output_as_clean() {
    let spec = checkstyle_like_spec();
    let findings = parse_output(&spec, "", "root").expect("empty output is a clean run");
    assert!(findings.is_empty());
}

#[test]
fn sarif_parser_errors_on_unparseable_input() {
    let spec = checkstyle_like_spec();
    let err = parse_output(&spec, "not json at all", "root")
        .expect_err("garbage must not be reported as clean");
    assert!(err.0.contains("checkstyle"), "message was {:?}", err.0);
}

/// A result with no location at all is not a finding: the tool is talking
/// about the run, not the code.
///
/// Verbatim from `tflint --format sarif` when its plugins were never
/// installed (`tflint --init` not run - the ordinary first-run and CI case):
/// the error arrives as a `tflint-errors` run whose single result carries no
/// `locations`. Read as a finding, its empty path matched nothing the run was
/// asked about and was narrowed away, so a tflint that never examined a file
/// reported every Terraform file clean. It has to be an error instead: we do
/// not know what else the tool would have said.
///
/// Keying on the location rather than the run name matters because a healthy
/// tflint run *also* emits a `tflint-errors` run - an empty one.
#[test]
fn sarif_parser_rejects_a_locationless_result() {
    let spec = tflint_like_spec();
    let tflint_errors = r#"{
      "version": "2.1.0",
      "runs": [
        { "tool": { "driver": { "name": "tflint" } }, "results": [] },
        { "tool": { "driver": { "name": "tflint-errors" } },
          "results": [
            { "ruleId": "application_error", "ruleIndex": 0, "level": "error",
              "message": { "text": "Failed to initialize plugins; Plugin \"aws\" not found. Did you run `tflint --init`?" } }
          ] }
      ]
    }"#;
    let err = parse_output(&spec, tflint_errors, "root")
        .expect_err("a locationless result is the tool failing, not a finding");
    assert!(
        err.0.contains("tflint") && err.0.contains("Plugin \"aws\" not found"),
        "the tool's own error text must reach the message: {}",
        err.0
    );
}

/// The empty `tflint-errors` run a healthy tflint emits is not an error:
/// there is no result to object to.
#[test]
fn sarif_parser_accepts_an_empty_error_run() {
    let spec = tflint_like_spec();
    let sarif = r#"{"runs":[
        {"tool":{"driver":{"name":"tflint"}},"results":[]},
        {"tool":{"driver":{"name":"tflint-errors"}},"results":[]}
    ]}"#;
    let findings = parse_output(&spec, sarif, "root").expect("an empty run is clean");
    assert!(findings.is_empty());
}

/// A location without a usable artifact uri is the same hole as a locationless
/// result: an empty path that narrowing drops without a trace.
#[test]
fn sarif_parser_rejects_a_location_without_a_uri() {
    let spec = checkstyle_like_spec();
    for locations in [
        // An empty locations array.
        r#""locations": []"#,
        // A location with no physicalLocation.
        r#""locations": [{"logicalLocations": []}]"#,
        // A physicalLocation whose artifactLocation has no uri.
        r#""locations": [{"physicalLocation": {"region": {"startLine": 3}}}]"#,
    ] {
        let sarif = format!(
            r#"{{"runs":[{{"results":[{{"level":"error","message":{{"text":"m"}},"ruleId":"r",{locations}}}]}}]}}"#
        );
        let err = parse_output(&spec, &sarif, "root")
            .expect_err("a result that cannot be placed is not a finding");
        assert!(
            err.0.contains("checkstyle"),
            "message was {:?} for {locations}",
            err.0
        );
    }
}

/// A result with a location but no `region` is a real finding pointing at the
/// whole file: tflint's `terraform_required_version` arrives exactly so.
/// It defaults to line 1 rather than being mistaken for a locationless error.
#[test]
fn sarif_parser_defaults_a_regionless_location_to_line_one() {
    let spec = tflint_like_spec();
    let sarif = r#"{"runs":[{"results":[
        { "ruleId": "terraform_required_version", "level": "warning",
          "message": { "text": "terraform \"required_version\" attribute is required" },
          "locations": [ { "physicalLocation": { "artifactLocation": { "uri": "main.tf" } } } ] }
    ]}]}"#;
    let findings = parse_output(&spec, sarif, "root").expect("a region is optional");
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].file_path, "main.tf");
    assert_eq!(findings[0].line, 1);
    assert_eq!(findings[0].column, None);
}
