//! `tsc` output parser.

use super::support::*;
use crate::analysis::findings::Severity;
use crate::languages::runner::*;

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

/// tsc's severity word is read, not assumed.
///
/// The regex always matched `warning` as well as `error`, and every match was
/// reported as `Severity::Error`. The gate was unaffected - a tool finding
/// blocks whatever its severity - but the rendered line called a warning an
/// error, which the compiler never said. Display truth matters because the
/// user calibrates the tool's config from what drep shows.
#[test]
fn tsc_parser_reads_the_severity_word() {
    let spec = ToolSpec {
        name: "tsc",
        output_format: OutputFormat::Tsc,
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
