//! Unit tests for the diff module.
//!
//! Wired in via `#[cfg(test)] mod tests;` in the parent module. These files
//! were orphaned once — present on disk but reachable by no `mod` declaration,
//! so cargo never compiled them and appending invalid Rust did not fail the
//! build. If you add a file here, declare it in this directory's `mod.rs`.

mod changed_since;
mod current_commit_sha;
mod hunk_commands;
mod hunks;
mod staged_files;
pub(crate) mod support;
