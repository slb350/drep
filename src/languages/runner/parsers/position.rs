//! Compiler-style positions: `file:line:col: message` and
//! `file(line,col): code: message`.
//!
//! Both are line-oriented and deliberately *skip* lines they do not recognise,
//! because Go interleaves `# example.com/pkg` package headers among its
//! diagnostics; a parser that errored on those would report every Go package
//! unanalyzable.

use std::sync::LazyLock;

use regex::Regex;

use crate::analysis::findings::{Finding, Severity};
use crate::languages::spec::ToolSpec;

/// `[vet: ]./path/to/file.go:12:6: message` - the compiler-style position
/// that `go vet` and most Go tooling emit.
///
/// The optional `vet: ` prefix and the `^` anchor matter: Go interleaves
/// `# example.com/pkg` package headers, and we skip those by *not* matching.
static POSITION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:vet:\s*)?(?P<file>[^\s:][^:]*):(?P<line>\d+):(?P<col>\d+):\s*(?P<message>.+)$")
        .expect("POSITION regex compiles")
});

/// `src/app.ts(14,22): error TS2345: message` - the tsc shape.
static TSC: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^(?P<file>.+?)\((?P<line>\d+),(?P<col>\d+)\):\s*(?:error|warning)\s+(?P<code>TS\d+):\s*(?P<message>.+)$",
    )
    .expect("TSC regex compiles")
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

/// `file(line,col): error TS1234: message`.
pub(super) fn parse_tsc(spec: &ToolSpec, output: &str) -> Vec<Finding> {
    let mut findings = Vec::new();
    for line in output.lines() {
        let Some(caps) = TSC.captures(line.trim()) else {
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
        let code = caps.name("code").map(|m| m.as_str()).unwrap_or(spec.name);
        let message = caps
            .name("message")
            .map(|m| m.as_str().trim())
            .unwrap_or("");
        findings.push(Finding::deterministic(
            code.to_owned(),
            Severity::Error,
            file.to_owned(),
            line_num,
            Some(col),
            message.to_owned(),
            None,
        ));
    }
    findings
}
