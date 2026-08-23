//! Helpers shared by the integration tests that assert over config files.
//!
//! Integration tests are separate crates, so this is the only sharing point
//! they have short of the library's public API - and the reader below has
//! already been transcribed once. `src/test_support.rs` is the wrong home: it
//! is `pub(crate)` and holds mock-endpoint fixtures.

/// A UTF-8 file from the repository root.
pub fn read(relative: &str) -> String {
    let path = format!("{}/{relative}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path} must be readable: {e}"))
}

/// A config file from the repository root, with its comment lines stripped.
///
/// The generated files carry an explanatory comment above every key, so a
/// raw-text assertion would be satisfied by the comment describing a setting
/// as readily as by the setting.
pub fn without_comments(relative: &str) -> String {
    read(relative)
        .lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n")
}
