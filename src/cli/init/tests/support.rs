//! Shared fixtures for the `drep init` test suite.

use crate::cli::init::{HookKind, InitArgs};

/// `InitArgs` with every flag at its clap default.
///
/// One builder rather than one per file: the struct has ten fields, and a new
/// one should be a single compile error here instead of a dozen across the
/// suite.
pub(super) fn args() -> InitArgs {
    InitArgs {
        path: std::path::PathBuf::from("."),
        provider: None,
        model: None,
        endpoint: None,
        hooks: HookKind::PrePush,
        force: false,
        no_gitignore: false,
        non_interactive: false,
        interactive: false,
    }
}

/// A store path inside `dir`, never the developer's real one.
///
/// `run_with` reads the store to decide whether a provider's key is already
/// held, so the real one would make the rendered `drep.toml` depend on whose
/// machine the suite ran on - and would be written to.
pub(super) fn auth_path(dir: &tempfile::TempDir) -> std::path::PathBuf {
    dir.path().join("auth.toml")
}

/// Render a one-provider `drep.toml`, the way the flag path does.
///
/// The tests used to call a `render` overload that existed only for them, and
/// which hardcoded `key_in_store: false` - so they could not see the flag
/// path's real behaviour. This builds the same `Choice` production builds.
pub(super) fn render_one(
    preset: &'static crate::cli::init::presets::LlmPreset,
    model: &str,
    endpoint: &str,
) -> String {
    crate::cli::init::config_file::render_chain(&[crate::cli::init::config_file::Choice {
        preset,
        model: model.to_string(),
        endpoint: endpoint.to_string(),
        key_in_store: false,
    }])
}
