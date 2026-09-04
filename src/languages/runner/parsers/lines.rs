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
            // The suggestion is `command` minus its last element, then `-w
            // {path}`. For gofmt the dropped element is `-l`.
            //
            // `split_last` rather than `command[..len - 1]`: that indexing
            // underflows on an empty argv, and `parse_output` is public, so a
            // spec naming this format with no command panicked the gate rather
            // than reporting a finding. A spec with fewer than two elements
            // has no rewrite command to name, so it gets no suggestion instead
            // of a malformed one.
            let suggest = match spec.command.split_last() {
                Some((_, base)) if !base.is_empty() => {
                    Some(format!("Run `{} -w {path}`", base.join(" ")))
                }
                _ => None,
            };
            Finding::deterministic(
                spec.name.to_owned(),
                Severity::Error,
                path.to_owned(),
                1,
                None,
                format!("{}: file is not formatted", spec.name),
                suggest,
            )
        })
        .collect()
}
