//! Acceptance tests for `drep init` (Part B).
//!
//! The tests run real git where the contract is about hooks, so a fixture
//! helper that initialises a real repository lives next to them; tests that
//! exercise `render` or `write` run against a plain `TempDir`.

mod b_codex;
mod b_config_file;
mod b_gitignore;
mod b_hook_exec;
mod b_hook_install_safety;
mod b_hooks;
mod b_plan;
mod b_presets;
mod b_reinit;
mod b_run;
mod support;
