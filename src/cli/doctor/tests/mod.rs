//! Acceptance tests for `drep doctor` (Part A).
//!
//! Each test runs `run_to` against a `TempDir` and asserts on the captured
//! string. No subprocess - the command takes a `&mut dyn Write` precisely so
//! the tests can read what would otherwise go to stdout.

mod a_codex;
mod a_key_command;
mod a_languages;
mod a_llm_section;
mod a_no_files;
mod a_skipped_vs_missing;
mod a_special_cases;
