//! Unit tests for the analysis module.
//!
//! Wired in via `#[cfg(test)] mod tests;` in the parent module. These files
//! were orphaned once — present on disk but reachable by no `mod` declaration,
//! so cargo never compiled them. If you add a file here, declare it in this
//! directory's `mod.rs`.

mod code_quality;
mod code_quality_failures;
mod code_quality_multi;
mod code_quality_truncation;
mod payload;
mod prompt;
mod result;
pub(crate) mod support;
