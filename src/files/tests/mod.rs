//! Unit tests for the file-target policy.
//!
//! Wired in via `#[cfg(test)] mod tests;` in the parent module. These files
//! were orphaned once — present on disk but reachable by no `mod` declaration,
//! so cargo never compiled them and appending invalid Rust did not fail the
//! build. If you add a file here, declare it in this directory's `mod.rs`.

mod expand_named;
mod expand_paths;
mod ignored_dir;
mod predicates;
mod walk_targets;
