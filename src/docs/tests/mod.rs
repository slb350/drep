//! Tests for the markdown checks.
//!
//! Every file here must be declared below. Rust silently ignores a test file
//! no `mod` points at, and four such files once held 31 tests that never
//! compiled while the count looked right.

mod analyze;
mod blocks;
mod fence;
mod lines;
mod links;
mod vocabulary;

use std::path::Path;

use crate::analysis::findings::Finding;
use crate::docs::{Check, analyze};

/// Analyze `content` as `doc.md`.
fn run(content: &str) -> Vec<Finding> {
    analyze(Path::new("doc.md"), content)
}

/// Every finding of one kind, in the order `analyze` returned them.
fn of_kind(content: &str, check: Check) -> Vec<Finding> {
    run(content)
        .into_iter()
        .filter(|f| f.kind == check.as_str())
        .collect()
}

/// The `(line, column)` of every finding of one kind.
fn positions(content: &str, check: Check) -> Vec<(u32, u32)> {
    of_kind(content, check)
        .into_iter()
        .map(|f| {
            (
                f.line,
                f.column.expect("every doc finding carries a column"),
            )
        })
        .collect()
}

/// Assert that `check` fires exactly once, at `line`:`column`.
#[track_caller]
fn fires_once_at(content: &str, check: Check, line: u32, column: u32) {
    assert_eq!(
        positions(content, check),
        vec![(line, column)],
        "{} over {content:?}",
        check.as_str()
    );
}

/// Assert that `check` does not fire at all.
#[track_caller]
fn silent(content: &str, check: Check) {
    assert_eq!(
        positions(content, check),
        Vec::new(),
        "{} should not fire over {content:?}",
        check.as_str()
    );
}

/// A line of `n` `x`s - for the length-boundary tests.
fn wide(n: usize) -> String {
    "x".repeat(n)
}
