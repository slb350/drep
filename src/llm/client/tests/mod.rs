//! Unit tests for `LlmClient`.
//!
//! Wired in via `#[cfg(test)] mod tests;` in the parent module. These files
//! were orphaned once — present on disk but reachable by no `mod`
//! declaration, so cargo never compiled them and appending invalid Rust did
//! not fail the build. If you add a file here, declare it in this directory's
//! `mod.rs`.

mod complete_json;
mod construction;
