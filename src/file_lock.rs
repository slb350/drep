//! One process-and-file lock for atomic local state transitions.

use std::fs::{File, OpenOptions};
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

// Advisory file locking can be process-associated, so a process mutex closes
// the same-process gap while the file lock serializes independent processes.
static PROCESS_LOCK: Mutex<()> = Mutex::new(());

/// A guard held until both the in-process mutex and advisory file lock drop.
pub(crate) struct ExclusiveLock {
    _file: File,
    _process: MutexGuard<'static, ()>,
}

/// Exclusively lock the path, whose parent must already exist.
pub(crate) fn exclusive(path: &Path) -> std::io::Result<ExclusiveLock> {
    let _process = PROCESS_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path)?;
    fs2::FileExt::lock_exclusive(&file)?;
    Ok(ExclusiveLock {
        _file: file,
        _process,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locking_does_not_create_a_missing_parent() {
        let base = tempfile::tempdir().expect("tempdir");
        let parent = base.path().join("missing");

        let error = exclusive(&parent.join("state.lock"))
            .err()
            .expect("a caller must create its own parent");

        assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
        assert!(!parent.exists());
    }
}
