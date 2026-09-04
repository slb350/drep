//! `run_tool`'s exit-status verdicts: when a non-zero exit is `Unavailable` and when it is findings.

use tempfile::TempDir;

use super::support::*;
use crate::languages::runner::*;

/// A tool that exits non-zero having produced no diagnostics is `Unavailable`,
/// not a clean `Ok`.
///
/// The exit code alone is not a verdict - ruff and clippy exit non-zero
/// *because* they found issues - but a non-zero exit with nothing on the
/// diagnostics stream means the tool did not run: bad config, crash, bad
/// invocation. Reporting that as `Ok` with zero findings is the
/// "unavailable is not a pass" failure this module exists to prevent.
#[tokio::test]
async fn a_silent_non_zero_exit_is_unavailable_not_a_clean_pass() {
    let dir = TempDir::new().unwrap();
    let bin = dir.path().join("failtool");
    write_executable(&bin, "#!/bin/sh\necho 'fatal: bad config' >&2\nexit 2\n");
    std::fs::write(dir.path().join("pyproject.toml"), "").unwrap();

    // `lines`, not `json`. An empty stdout is not valid JSON, so a `json`
    // spec reaches `Unavailable` through the *parse-failure* path and the test
    // passes without ever exercising the exit-status rule - which is exactly
    // what happened on the first draft. The `lines` parser accepts empty input
    // as zero findings, so only the new rule can produce `Unavailable` here.
    let spec = ToolSpec {
        name: "failtool",
        command: &["failtool"],
        local_paths: &["failtool"],
        config_files: &["pyproject.toml"],
        output_format: OutputFormat::Lines,
        diagnostics_stream: DiagnosticsStream::Stdout,
        ..ToolSpec::default()
    };

    let outcome = run_tool(&spec, dir.path(), &["a.py".to_owned()]).await;
    assert_eq!(
        outcome.status,
        ToolStatus::Unavailable,
        "a silent non-zero exit must not read as a clean pass, got {outcome:?}"
    );
    assert!(
        outcome.detail.contains("fatal: bad config"),
        "the other stream carries the real error and must reach the detail: {}",
        outcome.detail
    );
    // The stub's own exit code, not a placeholder. "exited without producing
    // diagnostics" tells the reader nothing they can act on, and the code is
    // the one part of the sentence that distinguishes a crash from a
    // deliberate refusal. The mutation gate found this: `exit_word` returning
    // a constant, or the empty string, passed every other assertion here.
    assert!(
        outcome.detail.contains("exited 2 "),
        "the tool's own exit code must reach the diagnostic: {}",
        outcome.detail
    );
}

/// A non-zero exit whose diagnostics are *unrecognisable* to a skipping
/// parser is `Unavailable`, not a clean pass.
///
/// The position/tsc/MSBuild parsers skip lines they do not recognise by
/// design, because their tools interleave chatter among the diagnostics. The
/// hole that opens: a run whose every line is an error of a shape the parser
/// does not know - the canonical case is `dotnet format --no-restore` on an
/// un-restored checkout, where MSBuild answers
/// `x.csproj : error NETSDK1004: Assets file 'project.assets.json' not found`
/// with no position - parses as zero findings on a non-empty stream, and the
/// empty-diagnostics guard above cannot see it. Every checked file came back
/// "No issues found" while the tool never examined one.
///
/// The JSON-shaped parsers cannot reach this state: output they do not
/// recognise is a parse error, which is already `Unavailable`. Only the
/// skipping formats need the second guard.
#[tokio::test]
async fn a_nonzero_exit_with_only_unmatched_lines_is_unavailable_for_skip_parsers() {
    for (format, stream, error_line) in [
        (
            OutputFormat::Msbuild,
            DiagnosticsStream::Stdout,
            "src/x.csproj : error NETSDK1004: Assets file 'project.assets.json' not found. Run a NuGet package restore to generate this file.",
        ),
        (
            OutputFormat::Tsc,
            DiagnosticsStream::Stdout,
            "error TS5023: Unknown compiler option '--strictNullChecking'.",
        ),
        (
            OutputFormat::Position,
            DiagnosticsStream::Stderr,
            "go: cannot find main module; see 'go help modules'",
        ),
    ] {
        let dir = TempDir::new().unwrap();
        let bin = dir.path().join("errtool");
        let emit = if stream == DiagnosticsStream::Stderr {
            ">&2"
        } else {
            ""
        };
        write_executable(
            &bin,
            format!("#!/bin/sh\nprintf '%s\\n' '{error_line}' {emit}\nexit 1\n"),
        );
        std::fs::write(dir.path().join("marker"), "").unwrap();

        let spec = ToolSpec {
            name: "errtool",
            command: &["errtool"],
            local_paths: &["errtool"],
            config_files: &["marker"],
            output_format: format,
            diagnostics_stream: stream,
            accepts_files: false,
            ..ToolSpec::default()
        };

        let outcome = run_tool(&spec, dir.path(), &["a.x".to_owned()]).await;
        assert_eq!(
            outcome.status,
            ToolStatus::Unavailable,
            "{format:?} exiting 1 with only unrecognised lines must not be a clean pass"
        );
        assert!(
            outcome.detail.contains("error") || outcome.detail.contains("go:"),
            "the unmatched lines are the diagnostic and must reach the detail: {}",
            outcome.detail
        );
        assert!(
            outcome.detail.contains("exited 1 "),
            "{format:?}: the tool's own exit code must reach the diagnostic: {}",
            outcome.detail
        );
    }
}

/// The guard's other half: a skipping parser that *did* read findings out of
/// the stream is an ordinary `Ok`, non-zero exit or not.
///
/// `dotnet format --verify-no-changes` exits 1 precisely when violations
/// exist, and those violations parse. Mistaking that for the failure above
/// would report a working tool as broken on every dirty C# file.
#[tokio::test]
async fn a_nonzero_exit_with_parsed_findings_is_still_ok() {
    let dir = TempDir::new().unwrap();
    let bin = dir.path().join("dotnet");
    let reported = dir.path().join("A.cs");
    write_executable(
        &bin,
        format!(
            "#!/bin/sh\nprintf '%s\\n' '{}(3,5): error WHITESPACE: Fix it [{}/cs.csproj]'\nexit 1\n",
            reported.to_string_lossy(),
            dir.path().to_string_lossy()
        ),
    );
    std::fs::write(dir.path().join("marker"), "").unwrap();

    let spec = ToolSpec {
        name: "dotnet format",
        command: &["dotnet"],
        local_paths: &["dotnet"],
        config_files: &["marker"],
        output_format: OutputFormat::Msbuild,
        diagnostics_stream: DiagnosticsStream::Stdout,
        accepts_files: false,
        ..ToolSpec::default()
    };

    let outcome = run_tool(&spec, dir.path(), &["A.cs".to_owned()]).await;
    assert_eq!(
        outcome.status,
        ToolStatus::Ok,
        "parseable violations on a non-zero exit are the findings path, got {outcome:?}"
    );
    assert_eq!(outcome.findings.len(), 1);
}
