//! MSBuild (`dotnet format`) output parser.

use super::support::*;
use crate::analysis::findings::Severity;
use crate::languages::runner::*;

/// Verbatim from `dotnet format --verify-no-changes` on .NET SDK 8.0.
fn msbuild_output() -> &'static str {
    "/tmp/cs/Program.cs(2,8): error WHITESPACE: Fix whitespace formatting. Replace 1 characters with '\\n'. [/tmp/cs/cs.csproj]\n/tmp/cs/Program.cs(4,9): error WHITESPACE: Fix whitespace formatting. Delete 4 characters. [/tmp/cs/cs.csproj]\n"
}

#[test]
fn msbuild_parser_reads_the_verified_fixture() {
    let spec = dotnet_format_like_spec();
    let findings = parse_output(&spec, msbuild_output(), "root").expect("msbuild output parses");
    assert_eq!(findings.len(), 2);
    assert_eq!(findings[0].file_path, "/tmp/cs/Program.cs");
    assert_eq!(findings[0].kind, "WHITESPACE");
    assert_eq!(findings[0].line, 2);
    assert_eq!(findings[0].column, Some(8));
    assert_eq!(findings[0].severity, Severity::Error);
    assert_eq!(
        findings[0].message,
        "Fix whitespace formatting. Replace 1 characters with '\\n'."
    );
    assert_eq!(findings[1].line, 4);
    assert_eq!(findings[1].column, Some(9));
    assert_eq!(
        findings[1].message,
        "Fix whitespace formatting. Delete 4 characters."
    );
}

/// The ` [project.csproj]` suffix names the project, not the problem, and
/// MSBuild appends it to every diagnostic. Left on, every message in every
/// C# project ends with the same path - noise that pushes the actual advice
/// off the end of a terminal line.
#[test]
fn msbuild_parser_strips_the_project_suffix_from_the_message() {
    let spec = dotnet_format_like_spec();
    let findings = parse_output(&spec, msbuild_output(), "root").unwrap();
    for finding in &findings {
        assert!(
            !finding.message.contains("csproj"),
            "project suffix survived: {:?}",
            finding.message
        );
        assert!(
            !finding.message.contains('['),
            "bracketed suffix survived: {:?}",
            finding.message
        );
    }
}

/// Bracketed text *inside* the message belongs to the message; only the
/// final bracketed suffix is the project name. A greedy strip would eat
/// meaningful text on any diagnostic that mentions an attribute or array.
#[test]
fn msbuild_parser_keeps_bracketed_text_that_is_not_the_suffix() {
    let spec = dotnet_format_like_spec();
    let findings = parse_output(
        &spec,
        "/cs/A.cs(3,5): error IDE0059: Value assigned to [config] is never used [/cs/cs.csproj]\n",
        "root",
    )
    .unwrap();
    assert_eq!(
        findings[0].message,
        "Value assigned to [config] is never used"
    );
}

/// MSBuild interleaves restore and build chatter among the diagnostics.
/// Erroring on those would report every C# project unanalyzable, so lines
/// that do not match the diagnostic shape are skipped, not failed.
#[test]
fn msbuild_parser_skips_unrecognised_lines_instead_of_erroring() {
    let spec = dotnet_format_like_spec();
    let input = "Restore complete (1.2s)\n  Determining projects to restore...\n/cs/A.cs(3,5): error WHITESPACE: Fix it [/cs/cs.csproj]\nBuild succeeded.\n    0 Warning(s)\n";
    let findings = parse_output(&spec, input, "root").expect("chatter is not an error");
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].kind, "WHITESPACE");
    assert_eq!(findings[0].message, "Fix it");
}

/// Empty stdout is a clean run: `dotnet format` with nothing to fix prints
/// nothing but the chatter this parser already skips.
#[test]
fn msbuild_parser_empty_and_whitespace_output_yield_no_findings() {
    let spec = dotnet_format_like_spec();
    assert!(parse_output(&spec, "", "root").unwrap().is_empty());
    assert!(parse_output(&spec, "  \n\t ", "root").unwrap().is_empty());
}

/// Every severity word the regex admits, one assertion per branch: `error`,
/// `warning` and `info` carry different weights and a collapsed arm would
/// misreport a warning as the error that blocks a commit.
#[test]
fn msbuild_severity_mapping_covers_every_branch() {
    let spec = dotnet_format_like_spec();
    for (word, code, expected) in [
        ("error", "WHITESPACE", Severity::Error),
        ("warning", "CS0168", Severity::Warning),
        ("info", "IDE0059", Severity::Info),
    ] {
        let line = format!("/cs/A.cs(1,1): {word} {code}: m [/cs/cs.csproj]\n");
        let findings = parse_output(&spec, &line, "root").unwrap();
        assert_eq!(findings[0].severity, expected, "severity word {word}");
        assert_eq!(findings[0].kind, code);
    }
}

/// A diagnostic without the project suffix still parses: the suffix is on
/// every real line but is not part of the grammar, and requiring it would
/// drop any diagnostic MSBuild emits bare.
#[test]
fn msbuild_parser_accepts_a_diagnostic_without_the_project_suffix() {
    let spec = dotnet_format_like_spec();
    let findings = parse_output(
        &spec,
        "/cs/A.cs(7,3): warning CS0168: declared but never used\n",
        "root",
    )
    .unwrap();
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].message, "declared but never used");
}

/// Diagnostics in two files both land: a "first entry only" regression must
/// not pass.
#[test]
fn msbuild_parser_reads_findings_in_every_file() {
    let spec = dotnet_format_like_spec();
    let input = "/cs/A.cs(1,1): error WHITESPACE: m1 [/cs/cs.csproj]\n/cs/B.cs(2,3): warning CS0168: m2 [/cs/cs.csproj]\n";
    let findings = parse_output(&spec, input, "root").unwrap();
    assert_eq!(findings.len(), 2);
    assert_eq!(findings[0].file_path, "/cs/A.cs");
    assert_eq!(findings[1].file_path, "/cs/B.cs");
    assert_eq!(findings[1].line, 2);
}

/// The suffix strip only eats a *project* path. A message that legitimately
/// ends in bracketed text keeps it: MSBuild appends the project file it was
/// building, always a `.csproj`/`.sln` path, and anything else in brackets
/// belongs to the message.
#[test]
fn msbuild_parser_strips_only_a_project_suffix() {
    let spec = dotnet_format_like_spec();
    let input = "/cs/A.cs(3,5): error IDE0059: Unnecessary assignment to [field] [/cs/cs.csproj]\n/cs/A.cs(4,5): error IDE0059: Unnecessary assignment to [field]\n";
    let findings = parse_output(&spec, input, "root").unwrap();
    assert_eq!(findings.len(), 2);
    assert_eq!(findings[0].message, "Unnecessary assignment to [field]");
    assert_eq!(
        findings[1].message, "Unnecessary assignment to [field]",
        "a trailing bracket that is not a project file stays"
    );
}
