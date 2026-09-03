//! `gofmt -l`-style output: one path per line, each naming a file to fix.

use crate::analysis::findings::{Finding, Severity};
use crate::languages::spec::ToolSpec;

/// `gofmt -l` prints one path per line: each non-blank line is a file that
/// needs formatting.
pub(super) fn parse_lines(spec: &ToolSpec, output: &str) -> Vec<Finding> {
    output
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let path = line.trim();
            // Build the suggestion from `command` minus its last element, then
            // append `-w {path}`. For gofmt the removed element is `-l`.
            let suggest = format!(
                "Run `{base} -w {path}`",
                base = spec.command[..spec.command.len() - 1].join(" "),
            );
            Finding::deterministic(
                spec.name.to_owned(),
                Severity::Error,
                path.to_owned(),
                1,
                None,
                format!("{}: file is not formatted", spec.name),
                Some(suggest),
            )
        })
        .collect()
}
