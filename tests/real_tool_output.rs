//! The parsers, against output captured from the real tools.
//!
//! The unit tests in `runner.rs` use hand-written fixtures, which prove the
//! parser matches its spec but not that the spec matches reality. These
//! samples were captured by running ruff 0.16, gofmt and go vet 1.21 against
//! deliberately broken files, so a tool changing its output shape fails here
//! rather than silently reporting every file clean.

use drep::analysis::findings::Severity;
use drep::languages::definitions::{
    CPPCHECK, CREDO, DOTNET_FORMAT, GO_VET, GOFMT, HADOLINT, PHPCS, RUBOCOP, RUFF, SHELLCHECK,
    SQLFLUFF, SWIFTLINT, TFLINT,
};
use drep::languages::runner::parse_output;
use drep::languages::spec::ToolSpec;

/// Captured from `ruff check --output-format json` on a file with two unused
/// imports and one unused local. Trimmed to the fields the parser reads.
const RUFF_JSON: &str = r#"[
  {"code":"F401","filename":"/tmp/real/bad.py",
   "fix":{"applicability":"safe","message":"Remove unused import: `os`"},
   "location":{"column":8,"row":1},"message":"`os` imported but unused"},
  {"code":"F401","filename":"/tmp/real/bad.py",
   "fix":{"applicability":"safe","message":"Remove unused import: `sys`"},
   "location":{"column":8,"row":2},"message":"`sys` imported but unused"},
  {"code":"F841","filename":"/tmp/real/bad.py",
   "fix":{"applicability":"unsafe","message":"Remove assignment to unused variable `x`"},
   "location":{"column":5,"row":6},"message":"Local variable `x` is assigned to but never used"}
]"#;

/// Captured from `gofmt -l .` with one badly formatted file.
const GOFMT_LINES: &str = "unformatted.go\n";

/// Captured from `go vet ./...`, which writes to stderr. Note the real shape
/// carries no `./` prefix and no package header in this case - the parser has
/// to handle both that and the header-bearing form.
const GO_VET_STDERR: &str =
    "main.go:6:14: fmt.Printf format %d has arg \"not an int\" of wrong type string\n";

#[test]
fn ruff_real_output_parses() {
    let findings = parse_output(&RUFF, RUFF_JSON, "fallback.py").expect("ruff output parses");

    assert_eq!(findings.len(), 3, "one finding per ruff diagnostic");

    let first = &findings[0];
    assert_eq!(first.kind, "F401");
    assert_eq!(first.severity, Severity::Error, "tool findings block");
    assert_eq!(first.file_path, "/tmp/real/bad.py");
    assert_eq!(first.line, 1);
    assert_eq!(first.column, Some(8));
    assert_eq!(first.message, "`os` imported but unused");
    assert_eq!(
        first.suggestion.as_deref(),
        Some("Remove unused import: `os`"),
        "ruff's fix.message is the suggestion"
    );

    assert_eq!(findings[2].kind, "F841");
    assert_eq!(findings[2].line, 6);
}

#[test]
fn gofmt_real_output_parses() {
    let findings = parse_output(&GOFMT, GOFMT_LINES, "fallback.go").expect("gofmt output parses");

    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].file_path, "unformatted.go");
    assert_eq!(findings[0].line, 1, "a formatting complaint is file-level");
    assert_eq!(findings[0].severity, Severity::Error);
    let suggestion = findings[0].suggestion.as_deref().unwrap_or_default();
    assert!(
        suggestion.contains("-w") && suggestion.contains("unformatted.go"),
        "suggestion should be runnable, got {suggestion:?}"
    );
}

#[test]
fn go_vet_real_output_parses() {
    let findings = parse_output(&GO_VET, GO_VET_STDERR, "fallback.go").expect("go vet parses");

    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].file_path, "main.go");
    assert_eq!(findings[0].line, 6);
    assert_eq!(findings[0].column, Some(14));
    assert!(
        findings[0].message.starts_with("fmt.Printf format %d"),
        "message should survive intact, got {:?}",
        findings[0].message
    );
}

#[test]
fn go_vet_package_header_form_also_parses() {
    // The multi-package form interleaves `# pkg` headers among diagnostics.
    let with_header = "# example.com/bad\n./main.go:6:14: some diagnostic\n";
    let findings = parse_output(&GO_VET, with_header, "fallback.go").expect("parses");

    assert_eq!(findings.len(), 1, "the header is skipped, not parsed");
    assert_eq!(
        findings[0].file_path, "main.go",
        "the ./ prefix is stripped"
    );
}

#[test]
fn a_clean_run_produces_no_findings() {
    // ruff prints `[]` when it finds nothing. That must be zero findings, not
    // a parse error - the common case must not look like a broken tool.
    assert!(
        parse_output(&RUFF, "[]", "x.py")
            .expect("empty array parses")
            .is_empty()
    );
    assert!(
        parse_output(&GOFMT, "", "x.go")
            .expect("empty output parses")
            .is_empty()
    );
    assert!(
        parse_output(&GO_VET, "", "x.go")
            .expect("empty output parses")
            .is_empty()
    );
}

// ---------------------------------------------------------------------------
// The languages registered in 2.10, against output captured from each tool.
//
// These go through the REAL `ToolSpec` statics rather than the test doubles the
// unit suite uses. That is the whole point of them: a double hardcodes its own
// `output_format`, so a typo in the real static - `"shell"` for `"shellcheck"` -
// passes every unit test and fails in production with "no parser for output
// format". Only a test holding the shipped spec can see that.
// ---------------------------------------------------------------------------

/// Captured from `shellcheck -f json` on a four-line script.
const SHELLCHECK_JSON: &str = r#"[{"file":"t.sh","line":2,"endLine":2,"column":1,"endColumn":4,"level":"warning","code":2034,"message":"foo appears unused. Verify use (or export if used externally).","fix":null},
{"file":"t.sh","line":3,"endLine":3,"column":6,"endColumn":10,"level":"info","code":2086,"message":"Double quote to prevent globbing and word splitting.","fix":{"replacements":[]}}]"#;

/// Captured from `rubocop --format json` 1.90.0. Trimmed to the read fields.
const RUBOCOP_JSON: &str = r#"{"metadata":{"rubocop_version":"1.90.0"},"files":[{"path":"Sample.rb","offenses":[
{"severity":"warning","message":"Useless assignment to variable - `y`.","cop_name":"Lint/UselessAssignment","location":{"start_line":2,"start_column":3,"line":2,"column":3}}]}],
"summary":{"offense_count":1}}"#;

/// Captured from `phpcs --report=json` 3.7.2. Note `files` is an object keyed
/// by an absolute path, unlike every other tool here.
const PHPCS_JSON: &str = r#"{"totals":{"errors":1},"files":{"\/w\/Sample.php":{"errors":1,"warnings":0,"messages":[
{"message":"Opening brace should be on a new line","source":"Squiz.Functions.MultiLineFunctionDeclaration.BraceOnSameLine","severity":5,"fixable":true,"type":"ERROR","line":2,"column":18}]}}}"#;

/// Captured from `mix credo --format json` 1.7. The first issue's `column` is
/// genuinely null, which is what pins the None rather than Some(0).
const CREDO_JSON: &str = r##"{"issues":[
{"category":"design","check":"Credo.Check.Design.TagTODO","column":null,"column_end":null,"filename":"lib/demo.ex","line_no":2,"message":"Found a TODO tag in a comment: # TODO: fix this","priority":1,"scope":"Demo","trigger":"# TODO: fix this"}]}"##;

/// Captured from `sqlfluff lint --format json` 4.3.0. Trimmed of `fixes`.
const SQLFLUFF_JSON: &str = r#"[{"filepath":"migration.sql","violations":[
{"start_line_no":3,"start_line_pos":1,"code":"CP02","description":"Unquoted identifiers must be consistently lower case.","name":"capitalisation.identifiers","warning":false,"fixes":[]}]}]"#;

/// Captured from `dotnet format --verify-no-changes` on .NET SDK 8.0. The
/// trailing bracketed project path is on every real line.
const DOTNET_FORMAT_STDOUT: &str = "/tmp/cs/Program.cs(4,9): error WHITESPACE: Fix whitespace formatting. Delete 4 characters. [/tmp/cs/cs.csproj]\n";

/// Captured from `cppcheck --output-format=sarif` 2.21.0, which writes it to
/// stderr. Trimmed to one result.
const CPPCHECK_SARIF: &str = r#"{"version":"2.1.0","runs":[{"results":[
{"level":"error","locations":[{"physicalLocation":{"artifactLocation":{"uri":"sample.c"},"region":{"startColumn":6,"startLine":5,"endColumn":6,"endLine":5}}}],
"message":{"text":"Array 'a[5]' accessed at index 5, which is out of bounds."},"ruleId":"arrayIndexOutOfBounds"}]}]}"#;

/// Captured from `swiftlint lint --reporter sarif` 0.65.1. Its URI is
/// repo-relative, which is why SwiftLint's SARIF is usable where ktlint's
/// (bound to `%SRCROOT%`) was not.
const SWIFTLINT_SARIF: &str = r#"{"runs":[{"results":[
{"level":"error","locations":[{"physicalLocation":{"artifactLocation":{"uri":"Sources/Sample.swift"},"region":{"startColumn":9,"startLine":4}}}],
"message":{"text":"Variable name 'x' should be between 3 and 40 characters long"},"ruleId":"identifier_name"}]}]}"#;

/// Captured from `tflint --format sarif` 0.64.0.
const TFLINT_SARIF: &str = r#"{"runs":[{"results":[
{"ruleId":"terraform_required_providers","ruleIndex":0,"level":"warning","message":{"text":"Missing version constraint for provider \"aws\" in `required_providers`"},
"locations":[{"physicalLocation":{"artifactLocation":{"uri":"main.tf"},"region":{"startLine":1,"startColumn":1,"endLine":1,"endColumn":30}}}]}]}]}"#;

/// Captured from `hadolint --format sarif` 2.15.1.
const HADOLINT_SARIF: &str = r#"{"runs":[{"results":[
{"level":"warning","locations":[{"physicalLocation":{"artifactLocation":{"uri":"Dockerfile"},"region":{"startColumn":1,"startLine":1,"sourceLanguage":"dockerfile"}}}],
"message":{"text":"Using latest is prone to errors if the image will ever update. Pin the version explicitly to a release tag"},"ruleId":"DL3007"}]}]}"#;

/// One row of the per-tool table below: the shipped spec, the output
/// captured from the real tool, and the finding that output must produce.
type RealOutputCase<'a> = (
    &'a ToolSpec,
    &'a str,
    &'a str,
    u32,
    Option<u32>,
    &'a str,
    Severity,
);

/// Every newly registered tool parses its own real output through its shipped
/// spec, and lands the finding on the file, line and rule the tool named.
///
/// One table rather than ten functions: the assertion is identical in every
/// row, and a per-tool function would make adding the eleventh a copy-paste.
#[test]
fn every_registered_tool_parses_its_real_output() {
    let cases: &[RealOutputCase] = &[
        (
            &SHELLCHECK,
            SHELLCHECK_JSON,
            "t.sh",
            2,
            Some(1),
            "SC2034",
            Severity::Warning,
        ),
        (
            &RUBOCOP,
            RUBOCOP_JSON,
            "Sample.rb",
            2,
            Some(3),
            "Lint/UselessAssignment",
            Severity::Warning,
        ),
        (
            &PHPCS,
            PHPCS_JSON,
            "/w/Sample.php",
            2,
            Some(18),
            "Squiz.Functions.MultiLineFunctionDeclaration.BraceOnSameLine",
            Severity::Error,
        ),
        (
            &CREDO,
            CREDO_JSON,
            "lib/demo.ex",
            2,
            None,
            "Credo.Check.Design.TagTODO",
            Severity::Warning,
        ),
        (
            &SQLFLUFF,
            SQLFLUFF_JSON,
            "migration.sql",
            3,
            Some(1),
            "CP02",
            Severity::Error,
        ),
        (
            &DOTNET_FORMAT,
            DOTNET_FORMAT_STDOUT,
            "/tmp/cs/Program.cs",
            4,
            Some(9),
            "WHITESPACE",
            Severity::Error,
        ),
        (
            &CPPCHECK,
            CPPCHECK_SARIF,
            "sample.c",
            5,
            Some(6),
            "arrayIndexOutOfBounds",
            Severity::Error,
        ),
        (
            &SWIFTLINT,
            SWIFTLINT_SARIF,
            "Sources/Sample.swift",
            4,
            Some(9),
            "identifier_name",
            Severity::Error,
        ),
        (
            &TFLINT,
            TFLINT_SARIF,
            "main.tf",
            1,
            Some(1),
            "terraform_required_providers",
            Severity::Warning,
        ),
        (
            &HADOLINT,
            HADOLINT_SARIF,
            "Dockerfile",
            1,
            Some(1),
            "DL3007",
            Severity::Warning,
        ),
    ];

    for (spec, output, file, line, column, kind, severity) in cases {
        let findings = parse_output(spec, output, "fallback")
            .unwrap_or_else(|err| panic!("{} should parse its own output: {err}", spec.name));
        let first = findings
            .first()
            .unwrap_or_else(|| panic!("{} produced no finding from real output", spec.name));
        assert_eq!(&first.file_path, file, "{} file path", spec.name);
        assert_eq!(first.line, *line, "{} line", spec.name);
        assert_eq!(first.column, *column, "{} column", spec.name);
        assert_eq!(&first.kind, kind, "{} rule id", spec.name);
        assert_eq!(first.severity, *severity, "{} severity", spec.name);
    }
}

/// A clean run says nothing, and must not be read as an unparseable one.
///
/// Separate from the table above because the tools disagree about what "said
/// nothing" looks like on the wire, and a shared row cannot express that.
#[test]
fn every_registered_tool_reads_its_own_clean_run() {
    let cases: &[(&ToolSpec, &str)] = &[
        (&SHELLCHECK, "[]"),
        (&RUBOCOP, r#"{"files":[]}"#),
        (&PHPCS, r#"{"totals":{"errors":0},"files":{}}"#),
        (&CREDO, r#"{"issues":[]}"#),
        (&SQLFLUFF, "[]"),
        (&DOTNET_FORMAT, ""),
        (&CPPCHECK, r#"{"version":"2.1.0","runs":[{"results":[]}]}"#),
        (&SWIFTLINT, r#"{"runs":[{"results":[]}]}"#),
        (&TFLINT, r#"{"runs":[{"results":[]}]}"#),
        (&HADOLINT, r#"{"runs":[{"results":[]}]}"#),
    ];

    for (spec, output) in cases {
        let findings = parse_output(spec, output, "fallback")
            .unwrap_or_else(|err| panic!("{} clean run should parse: {err}", spec.name));
        assert!(
            findings.is_empty(),
            "{} reported a finding on a clean run",
            spec.name
        );
    }
}

/// Exactly two shipped tools write their diagnostics to stderr, and both are
/// verified: `go vet` streams there by design, and cppcheck writes its whole
/// SARIF document there while leaving stdout all but empty (22 bytes on the
/// capture above).
///
/// Pinned as a list rather than asserted per tool because the failure is
/// silent in the worst direction: reading the wrong stream finds no
/// diagnostics, and no diagnostics is reported as a clean file. A tool added
/// with the wrong stream would make drep pass every file in that language.
#[test]
fn only_the_verified_tools_read_stderr() {
    let mut stderr_tools: Vec<&str> = drep::languages::definitions::ALL_LANGUAGES
        .iter()
        .flat_map(|lang| lang.tools.iter())
        .filter(|spec| spec.diagnostics_stream == "stderr")
        .map(|spec| spec.name)
        .collect();
    // Sorted: registration order is `doctor`'s reporting order and is allowed
    // to change, which is not what this test is about.
    stderr_tools.sort_unstable();

    assert_eq!(
        stderr_tools,
        vec!["cppcheck", "cppcheck", "go vet"],
        "cppcheck is listed twice because C and C++ share the one spec"
    );
}

/// Every shipped tool names a stream the runner actually reads, and an output
/// format `parse_output` actually dispatches on.
///
/// `diagnostics_stream` is compared against a literal in `run_tool_at`, so a
/// typo silently selects stdout; `output_format` falls through to an error
/// that reports the file unanalyzable. Neither is caught by a parser test,
/// which supplies its own spec.
#[test]
fn every_shipped_tool_names_a_real_stream_and_format() {
    for lang in drep::languages::definitions::ALL_LANGUAGES {
        for spec in lang.tools {
            assert!(
                matches!(spec.diagnostics_stream, "stdout" | "stderr"),
                "{} names stream {:?}",
                spec.name,
                spec.diagnostics_stream
            );
            let err = parse_output(spec, "", "fallback");
            assert!(
                err.is_ok(),
                "{} format {:?} has no parser",
                spec.name,
                spec.output_format
            );
        }
    }
}
