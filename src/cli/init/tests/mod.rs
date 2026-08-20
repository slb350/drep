//! Acceptance tests for `drep init` (Part B).
//!
//! One `#[test]` per criterion in PHASE_SPEC.md. The tests run real git
//! where the criterion is about hooks (B10-B16), so a fixture helper that
//! initialises a real repository lives next to them; criteria that exercise
//! `render` or `write` run against a plain `TempDir`.

mod b_codex;
mod b_config_file;
mod b_gitignore;
mod b_hook_exec;
mod b_hooks;
mod b_plan;
mod b_presets;
mod b_reinit;
mod b_run;
mod support;
