//! The five output parsers, plus the unknown-format case.
//!
//! Wired in via `#[cfg(test)] mod tests;` in the parent module. These files were
//! orphaned once - present on disk but reachable by no `mod` declaration, so
//! cargo never compiled them and appending invalid Rust did not fail the build.
//! If you add a file here, declare it in this directory's `mod.rs`.

use super::support::*;
use crate::analysis::findings::Severity;
use crate::languages::runner::*;

// ---- parsers: lines ----

#[test]
fn lines_parser_yields_two_findings_for_two_paths() {
    let spec = gofmt_like_spec();
    let findings =
        parse_output(&spec, "a.go\nb.go\n", "root").expect("lines parser does not error");
    assert_eq!(findings.len(), 2);
    assert_eq!(findings[0].file_path, "a.go");
    assert_eq!(findings[0].line, 1);
    assert_eq!(findings[0].severity, Severity::Error);
    assert_eq!(findings[1].file_path, "b.go");
}

#[test]
fn lines_parser_skips_blank_lines() {
    let spec = gofmt_like_spec();
    let findings =
        parse_output(&spec, "\n\nonly.go\n\n", "root").expect("lines parser does not error");
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].file_path, "only.go");
}

#[test]
fn lines_parser_suggestion_includes_minus_w_and_path() {
    let spec = gofmt_like_spec();
    let findings = parse_output(&spec, "src/main.go\n", "root").unwrap();
    let suggestion = findings[0].suggestion.as_deref().unwrap();
    assert!(suggestion.contains("-w"), "suggestion was {suggestion:?}");
    assert!(
        suggestion.contains("src/main.go"),
        "suggestion was {suggestion:?}"
    );
}

// ---- parsers: json ----

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
    let spec = ToolSpec {
        name: "eslint",
        command: &["eslint"],
        local_paths: &[],
        config_files: &[".eslintrc"],
        output_format: "json",
        diagnostics_stream: "stdout",
        ..ToolSpec::default()
    };
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

// ---- parsers: position ----

#[test]
fn position_parser_strips_leading_dot_slash() {
    let spec = go_vet_like_spec();
    let findings = parse_output(&spec, "./main.go:12:6: undefined: foo", "root").unwrap();
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].file_path, "main.go");
    assert_eq!(findings[0].line, 12);
    assert_eq!(findings[0].column, Some(6));
}

#[test]
fn position_parser_skips_package_headers_keeps_following_diagnostic() {
    let spec = go_vet_like_spec();
    let input = "# example.com/pkg\n./main.go:4:2: bad\n";
    let findings = parse_output(&spec, input, "root").unwrap();
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].line, 4);
}

#[test]
fn position_parser_accepts_vet_prefix() {
    let spec = go_vet_like_spec();
    let findings = parse_output(&spec, "vet: ./a.go:1:1: msg", "root").unwrap();
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].file_path, "a.go");
    assert_eq!(findings[0].message, "msg");
}

// ---- parsers: tsc ----

#[test]
fn tsc_parser_extracts_code_line_column_and_file() {
    let spec = tsc_like_spec();
    let findings = parse_output(&spec, "src/app.ts(14,22): error TS2345: bad arg", "root").unwrap();
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].kind, "TS2345");
    assert_eq!(findings[0].file_path, "src/app.ts");
    assert_eq!(findings[0].line, 14);
    assert_eq!(findings[0].column, Some(22));
}

#[test]
fn tsc_parser_accepts_warning_codes() {
    let spec = tsc_like_spec();
    let findings = parse_output(
        &spec,
        "src/app.ts(2,3): warning TS6133: 'x' is declared",
        "root",
    )
    .unwrap();
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].kind, "TS6133");
}

#[test]
fn tsc_parser_skips_unrelated_noise_lines() {
    let spec = tsc_like_spec();
    let input = "some unrelated log line\nsrc/app.ts(1,1): error TS0001: bad\nanother noise\n";
    let findings = parse_output(&spec, input, "root").unwrap();
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].kind, "TS0001");
}

// ---- parsers: cargo ----

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

// ---- parsers: unknown format ----

#[test]
fn unknown_output_format_is_error_not_empty() {
    let spec = ToolSpec {
        output_format: "not-a-format",
        ..ToolSpec::default()
    };
    let err = parse_output(&spec, "anything", "root").unwrap_err();
    assert!(err.0.contains("not-a-format"));
}

// ---- parsers: sarif ----

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

// ---- parsers: ktlint ----

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

/// A `lines` spec with too short an argv gets no suggestion, rather than
/// panicking the gate.
///
/// The suggestion was built as `command[..len - 1]`, which underflows on an
/// empty argv because the index is a `usize`. `parse_output` is public, so a
/// spec naming this format with no command crashed the process instead of
/// reporting a finding - and a commit gate that panics is a commit gate that
/// blocks every commit.
#[test]
fn lines_parser_with_a_short_argv_omits_the_suggestion_instead_of_panicking() {
    for command in [&[][..], &["gofmt"][..]] {
        let spec = ToolSpec {
            name: "shortargv",
            command,
            output_format: "lines",
            ..ToolSpec::default()
        };
        let findings = parse_output(&spec, "unformatted.go\n", "root").expect("lines parse");
        assert_eq!(findings.len(), 1, "the finding itself still lands");
        assert_eq!(
            findings[0].suggestion, None,
            "there is no rewrite command to name"
        );
    }
}

/// The full argv still produces the rewrite suggestion.
#[test]
fn lines_parser_suggests_the_rewrite_when_the_argv_has_one() {
    let spec = ToolSpec {
        name: "gofmt",
        command: &["gofmt", "-l"],
        output_format: "lines",
        ..ToolSpec::default()
    };
    let findings = parse_output(&spec, "unformatted.go\n", "root").expect("lines parse");
    assert_eq!(
        findings[0].suggestion.as_deref(),
        Some("Run `gofmt -w unformatted.go`")
    );
}

/// tsc's severity word is read, not assumed.
///
/// The regex has always matched `warning` as well as `error`, and every match
/// was reported as `Severity::Error`, so a tsc warning blocked a commit under
/// `--fail-on error` while claiming to be an error it was not.
#[test]
fn tsc_parser_reads_the_severity_word() {
    let spec = ToolSpec {
        name: "tsc",
        output_format: "tsc",
        ..ToolSpec::default()
    };
    let output =
        "src/a.ts(1,1): error TS2345: an error\nsrc/b.ts(2,2): warning TS6133: a warning\n";
    let findings = parse_output(&spec, output, "root").expect("tsc parse");
    assert_eq!(findings.len(), 2);
    assert_eq!(findings[0].severity, Severity::Error);
    assert_eq!(findings[1].severity, Severity::Warning);
    assert_eq!(findings[1].kind, "TS6133");
}
