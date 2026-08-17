//! The parsers, against output captured from the real tools.
//!
//! The unit tests in `runner.rs` use hand-written fixtures, which prove the
//! parser matches its spec but not that the spec matches reality. These
//! samples were captured by running ruff 0.16, gofmt and go vet 1.21 against
//! deliberately broken files, so a tool changing its output shape fails here
//! rather than silently reporting every file clean.

use drep::analysis::findings::Severity;
use drep::languages::definitions::{GO_VET, GOFMT, RUFF};
use drep::languages::runner::parse_output;

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
