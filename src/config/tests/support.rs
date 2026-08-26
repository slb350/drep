//! Shared fixture for the config suite.

/// Write `body` to a fresh `drep.toml`-shaped path in `temp`.
pub(super) fn write_config(temp: &tempfile::TempDir, body: &str) -> std::path::PathBuf {
    let path = temp.path().join("drep.toml");
    std::fs::write(&path, body).expect("write config");
    path
}

/// Write `body` to a fresh `site.toml`-shaped path in `temp`.
///
/// A separate helper rather than a filename parameter on [`write_config`]: the
/// two files have different grammars and different error types, and a test that
/// handed site policy to a `drep.toml`-shaped path would still look like it had
/// passed.
pub(super) fn write_site(temp: &tempfile::TempDir, body: &str) -> std::path::PathBuf {
    let path = temp.path().join("site.toml");
    std::fs::write(&path, body).expect("write site policy");
    path
}
