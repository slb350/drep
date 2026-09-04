//! `lines` (`gofmt -l`) output parser.

use super::support::*;
use crate::analysis::findings::Severity;
use crate::languages::runner::*;

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
            output_format: OutputFormat::Lines,
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
        output_format: OutputFormat::Lines,
        ..ToolSpec::default()
    };
    let findings = parse_output(&spec, "unformatted.go\n", "root").expect("lines parse");
    assert_eq!(
        findings[0].suggestion.as_deref(),
        Some("Run `gofmt -w unformatted.go`")
    );
}
