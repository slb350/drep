//! Atomic file publication via a private sibling temporary file.
//!
//! Every persistent file drep writes (credentials, repo config, hooks, hook
//! backups, acknowledgements, model quirks cache, `.gitignore`) goes through
//! this helper so an interrupted write never leaves a truncated file behind
//! and a replacement never follows a destination symlink.

use std::io::Write;
use std::path::Path;

/// Whether publishing may replace an existing directory entry.
#[derive(Clone, Copy)]
pub(crate) enum Mode {
    /// Atomically replace any existing entry at the destination.
    Replace,
    /// Fail with `AlreadyExists` if any entry (including a dangling symlink)
    /// already occupies the destination.
    NoClobber,
}

/// The directory that holds a sibling temporary file for `path`.
pub(crate) fn parent_dir(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

/// Write `bytes` into a private sibling temporary file beside `path`, run
/// `prepare` on the temporary path (for example to set permissions), fsync it,
/// and publish it according to `mode`.
pub(crate) fn publish_with(
    path: &Path,
    bytes: &[u8],
    mode: Mode,
    prepare: impl FnOnce(&Path) -> std::io::Result<()>,
) -> std::io::Result<()> {
    let parent = parent_dir(path);
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(bytes)?;
    prepare(temporary.path())?;
    temporary.as_file().sync_all()?;
    match mode {
        Mode::Replace => temporary.persist(path),
        Mode::NoClobber => temporary.persist_noclobber(path),
    }
    .map(|_| ())
    .map_err(|err| err.error)
}

/// [`publish_with`] with no extra preparation step.
pub(crate) fn publish(path: &Path, bytes: &[u8], mode: Mode) -> std::io::Result<()> {
    publish_with(path, bytes, mode, |_| Ok(()))
}
