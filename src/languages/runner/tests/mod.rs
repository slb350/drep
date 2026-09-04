//! Unit tests for the deterministic tool runner.
//!
//! Every file in this directory must be declared below. These files were
//! orphaned once - present on disk but reachable by no `mod` declaration, so
//! cargo never compiled them and appending invalid Rust did not fail the
//! build. If you add a file here, declare it here.

mod parsers_cargo;
mod parsers_credo;
mod parsers_json;
mod parsers_ktlint;
mod parsers_lines;
mod parsers_msbuild;
mod parsers_phpcs;
mod parsers_position;
mod parsers_rubocop;
mod parsers_sarif;
mod parsers_shellcheck;
mod parsers_sqlfluff;
mod parsers_tsc;
mod resolve;
mod run_tool;
mod run_tool_exit_status;
mod run_tool_narrowing;
mod stream_detail;
pub(crate) mod support;
