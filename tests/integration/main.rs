//! Single integration test binary for `drep`.
//!
//! Cargo compiles and links every top-level file under `tests/` as an
//! independent crate against `libdrep` and its full dependency graph. Grouping
//! all integration suites under one root binary keeps every test intact while
//! eliminating six redundant compilations and links on every `cargo test` and
//! every mutation-testing build.

mod cli;
mod common;
mod fixture_git_isolation;
mod ground_truth_walk;
mod init_without_a_terminal;
mod published_hooks;
mod real_tool_output;
mod release_config;
