//! `position` (`go vet`) output parser.

use super::support::*;
use crate::languages::runner::*;

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

/// A Windows absolute path carries a drive-letter colon, and the position
/// parser must still see the diagnostic.
///
/// The file group forbade colons outright, so `C:\src\main.go:12:6: msg` did
/// not match - and because this parser skips what it cannot match, a Windows
/// `go vet` run lost every diagnostic and the gate passed clean.
#[test]
fn position_parser_reads_a_windows_drive_letter_path() {
    let spec = ToolSpec {
        name: "go vet",
        output_format: OutputFormat::Position,
        ..ToolSpec::default()
    };
    let findings = parse_output(
        &spec,
        r"C:\src\main.go:12:6: fmt.Printf format %d has arg of wrong type",
        "root",
    )
    .expect("position parse");
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].file_path, r"C:\src\main.go");
    assert_eq!(findings[0].line, 12);
    assert_eq!(findings[0].column, Some(6));
}
