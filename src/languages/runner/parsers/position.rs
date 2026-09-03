//! Compiler-style positions: `file:line:col: message`,
//! `file(line,col): code: message`, and MSBuild's severity-carrying variant
//! of the same shape.
//!
//! All three are line-oriented and deliberately *skip* lines they do not
//! recognise, because Go interleaves `# example.com/pkg` package headers
//! among its diagnostics and MSBuild interleaves restore and build chatter
//! among its own; a parser that errored on those would report every Go
//! package or C# project unanalyzable.

use std::sync::LazyLock;

use regex::{Captures, Regex};

use crate::analysis::findings::{Finding, Severity};
use crate::languages::spec::ToolSpec;

/// `[vet: ]./path/to/file.go:12:6: message` - the compiler-style position
/// that `go vet` and most Go tooling emit.
///
/// The optional `vet: ` prefix and the `^` anchor matter: Go interleaves
/// `# example.com/pkg` package headers, and we skip those by *not* matching.
///
/// The file group is lazy rather than colon-free. Forbidding a colon dropped
/// `C:\src\main.go:12:6: message` outright, and because this parser skips
/// what it cannot match, a Windows `go vet` run lost every diagnostic
/// silently and the gate passed. Laziness plus the mandatory `:line:col:`
/// suffix still resolves unambiguously - the first position-shaped suffix
/// wins - while a package header, having no such suffix, still fails to
/// match.
static POSITION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:vet:\s*)?(?P<file>\S.*?):(?P<line>\d+):(?P<col>\d+):\s*(?P<message>.+)$")
        .expect("POSITION regex compiles")
});

/// `src/app.ts(14,22): error TS2345: message` - the tsc shape.
static TSC: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^(?P<file>.+?)\((?P<line>\d+),(?P<col>\d+)\):\s*(?P<severity>error|warning)\s+(?P<code>TS\d+):\s*(?P<message>.+)$",
    )
    .expect("TSC regex compiles")
});

/// `/tmp/cs/Program.cs(2,8): error WHITESPACE: message [/tmp/cs/cs.csproj]` -
/// the MSBuild shape `dotnet format --verify-no-changes` writes to stdout.
///
/// Modeled on the tsc shape but parameterised where tsc is fixed: the
/// severity word is captured rather than assumed, and the code is any
/// identifier (`WHITESPACE`, `CS0168`, `IDE0059`) rather than `TS\d+`, so
/// neither regex can serve the other's tool.
///
/// The trailing ` [project]` group is optional and anchored to `$`, so it
/// strips exactly the last bracketed suffix - the project name MSBuild
/// appends to every diagnostic - and leaves bracketed text inside the
/// message alone.
static MSBUILD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^(?P<file>.+?)\((?P<line>\d+),(?P<col>\d+)\):\s*(?P<severity>error|warning|info)\s+(?P<code>[A-Za-z0-9_]+):\s*(?P<message>.*?)(?:\s+\[[^\]]*\])?$",
    )
    .expect("MSBUILD regex compiles")
});

/// `file:line:col: message`, skipping the package headers Go interleaves.
pub(super) fn parse_positions(spec: &ToolSpec, output: &str) -> Vec<Finding> {
    let mut findings = Vec::new();
    for line in output.lines() {
        let Some(caps) = POSITION.captures(line.trim()) else {
            // `# example.com/pkg` headers and blank lines are not diagnostics.
            continue;
        };
        let file = caps.name("file").map(|m| m.as_str()).unwrap_or("");
        let line_num = caps
            .name("line")
            .and_then(|m| m.as_str().parse::<u32>().ok())
            .unwrap_or(1);
        let col = caps
            .name("col")
            .and_then(|m| m.as_str().parse::<u32>().ok())
            .unwrap_or(1);
        let message = caps
            .name("message")
            .map(|m| m.as_str().trim())
            .unwrap_or("");
        findings.push(Finding::deterministic(
            spec.name.to_owned(),
            Severity::Error,
            file.strip_prefix("./").unwrap_or(file).to_owned(),
            line_num,
            Some(col),
            message.to_owned(),
            None,
        ));
    }
    findings
}

/// Extract one finding from every line `re` matches, skipping the rest.
///
/// `file`, `code` and `message` are mandatory named groups in both the tsc
/// and MSBuild regexes, so indexing cannot miss once `captures` has matched;
/// the `unwrap_or` fallbacks they replace were dead. `line` and `col` keep
/// theirs because an overflowing number really does fail `u32` parsing.
/// The severity word is the one place the two shapes disagree - tsc's regex
/// matches it without capturing, MSBuild's captures it - so it arrives as a
/// closure over the captures.
///
/// The message is trimmed because the MSBUILD regex's lazy `message` group
/// can leave a trailing space on a line without a project suffix.
fn parse_captured(
    output: &str,
    re: &Regex,
    severity: impl Fn(&Captures<'_>) -> Severity,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    for line in output.lines() {
        let Some(caps) = re.captures(line.trim()) else {
            // Restore chatter, package headers and blank lines are not
            // diagnostics.
            continue;
        };
        let line_num = caps["line"].parse::<u32>().ok().unwrap_or(1);
        let col = caps["col"].parse::<u32>().ok().unwrap_or(1);
        findings.push(Finding::deterministic(
            caps["code"].to_owned(),
            severity(&caps),
            caps["file"].to_owned(),
            line_num,
            Some(col),
            caps["message"].trim().to_owned(),
            None,
        ));
    }
    findings
}

/// `file(line,col): error TS1234: message`.
///
/// The severity word is read rather than assumed. The regex has always
/// matched `warning` as well as `error`, and every match was reported as an
/// error - so a tsc warning blocked a commit under `--fail-on error` while
/// claiming to be something it was not. On a scale where a warning blocks,
/// the gate gets switched off.
pub(super) fn parse_tsc(output: &str) -> Vec<Finding> {
    parse_captured(output, &TSC, |caps| {
        if &caps["severity"] == "warning" {
            Severity::Warning
        } else {
            Severity::Error
        }
    })
}

/// `file(line,col): severity CODE: message [project]`, skipping other lines.
///
/// Line-oriented and skip-on-no-match, exactly like `parse_tsc`: MSBuild
/// interleaves restore and build chatter among the diagnostics, and erroring
/// on those would report every C# project unanalyzable.
pub(super) fn parse_msbuild(output: &str) -> Vec<Finding> {
    parse_captured(output, &MSBUILD, |caps| msbuild_severity(&caps["severity"]))
}

/// MSBuild's severity word to drep severity.
///
/// The regex admits only `error`, `warning` and `info`, so the catch-all is
/// reached for `info` - kept as a match arm rather than falling through so
/// each of the three words stays separately observable.
fn msbuild_severity(word: &str) -> Severity {
    match word {
        "error" => Severity::Error,
        "warning" => Severity::Warning,
        _ => Severity::Info,
    }
}
