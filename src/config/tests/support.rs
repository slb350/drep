//! Shared fixture for the config suite.

/// Write `body` to a fresh `drep.toml`-shaped path in `temp`.
pub(super) fn write_config(temp: &tempfile::TempDir, body: &str) -> std::path::PathBuf {
    let path = temp.path().join("drep.toml");
    std::fs::write(&path, body).expect("write config");
    path
}
