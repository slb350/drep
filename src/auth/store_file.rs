//! Locked, atomic persistence for the credential store.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use super::{AuthError, AuthStore};

impl AuthStore {
    /// Read the store at `path`.
    ///
    /// A missing file is an empty store, which is the normal first-run state.
    /// Every other read or parse failure remains fatal so corruption never
    /// masquerades as a machine with no stored credentials.
    pub fn load(path: &Path) -> Result<Self, AuthError> {
        let content = match fs::read_to_string(path) {
            Ok(content) => content,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Self::new()),
            Err(err) => return Err(AuthError::Read(path.to_path_buf(), err)),
        };
        toml::from_str(&content)
            .map_err(|err: toml::de::Error| AuthError::Parse(path.to_path_buf(), err.to_string()))
    }

    /// Merge this snapshot's mutations into the latest store and publish it.
    ///
    /// A process mutex closes the gap left by advisory file locks within one
    /// process; the sibling lock file serializes independent drep processes.
    pub fn save(&self, path: &Path) -> Result<(), AuthError> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            ensure_dir_private(parent)?;
        }

        let lock_path = lock_path(path);
        let _lock = crate::file_lock::exclusive(&lock_path)
            .map_err(|err| AuthError::Lock(lock_path.clone(), err))?;

        let mut pending = self
            .pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut keys = Self::load(path)?.keys;
        for endpoint in pending.iter() {
            if let Some(key) = self.keys.get(endpoint) {
                keys.insert(endpoint.clone(), key.clone());
            } else {
                keys.remove(endpoint);
            }
        }
        let body = serialize_keys(keys)?;
        write_private(path, &body)?;
        pending.clear();
        Ok(())
    }
}

fn serialize_keys(keys: BTreeMap<String, String>) -> Result<String, AuthError> {
    let store = AuthStore {
        keys,
        ..AuthStore::default()
    };
    toml::to_string_pretty(&store).map_err(|err| AuthError::Serialize(err.to_string()))
}

fn lock_path(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .unwrap_or_else(|| std::ffi::OsStr::new("auth.toml"))
        .to_os_string();
    name.push(".lock");
    path.with_file_name(name)
}

/// Create `dir` if missing, narrowing it only when drep made it.
///
/// `DREP_AUTH_PATH` can name any path, so changing an existing parent's mode
/// could turn `/etc/drep.toml` into a request to chmod `/etc`. The store file's
/// own 0600 mode protects credentials inside a directory the user owns.
pub(crate) fn ensure_dir_private(dir: &Path) -> Result<(), AuthError> {
    ensure_dir_private_with(dir, create_private_dir, sync_directory)
}

fn ensure_dir_private_with(
    dir: &Path,
    create_component: impl FnMut(&Path) -> std::io::Result<()>,
    sync_directory: impl FnMut(&Path) -> std::io::Result<()>,
) -> Result<(), AuthError> {
    ensure_dir_private_with_metadata(
        dir,
        |path| fs::symlink_metadata(path),
        create_component,
        sync_directory,
    )
}

fn ensure_dir_private_with_metadata(
    dir: &Path,
    mut metadata: impl FnMut(&Path) -> std::io::Result<fs::Metadata>,
    mut create_component: impl FnMut(&Path) -> std::io::Result<()>,
    mut sync_directory: impl FnMut(&Path) -> std::io::Result<()>,
) -> Result<(), AuthError> {
    let mut missing = Vec::new();
    let mut cursor = dir;
    loop {
        match metadata(cursor) {
            Ok(_) => break,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                missing.push(cursor.to_path_buf());
            }
            Err(err) => return Err(AuthError::Write(dir.to_path_buf(), err)),
        }
        let Some(parent) = cursor
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        else {
            break;
        };
        cursor = parent;
    }

    for created in missing.iter().rev() {
        match create_component(created) {
            Ok(()) => {
                restrict(created, 0o700)?;
                sync_directory(created)
                    .map_err(|err| AuthError::Write(created.to_path_buf(), err))?;
                sync_directory(temporary_parent(created))
                    .map_err(|err| AuthError::Write(created.to_path_buf(), err))?;
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                require_directory(created)?;
            }
            Err(err) => return Err(AuthError::Write(created.to_path_buf(), err)),
        }
    }
    require_directory(dir)
}

fn create_private_dir(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;

        let mut builder = fs::DirBuilder::new();
        builder.mode(0o700).create(path)
    }

    #[cfg(not(unix))]
    {
        fs::create_dir(path)
    }
}

fn require_directory(path: &Path) -> Result<(), AuthError> {
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_dir() => Ok(()),
        Ok(_) => Err(AuthError::Write(
            path.to_path_buf(),
            std::io::Error::from(std::io::ErrorKind::NotADirectory),
        )),
        Err(err) => Err(AuthError::Write(path.to_path_buf(), err)),
    }
}

/// Write through a random private sibling and atomically replace `path`.
///
/// The temporary starts private, is synced before publication, and has a
/// random exclusively-created name. That avoids a world-readable write window,
/// partial files after a crash, and predictable sibling-symlink attacks.
fn write_private(path: &Path, body: &str) -> Result<(), AuthError> {
    write_private_with(path, body, sync_directory)
}

fn write_private_with(
    path: &Path,
    body: &str,
    sync_parent: impl FnOnce(&Path) -> std::io::Result<()>,
) -> Result<(), AuthError> {
    let parent = temporary_parent(path);
    let mut temporary = tempfile::NamedTempFile::new_in(parent)
        .map_err(|err| AuthError::Write(path.to_path_buf(), err))?;
    temporary
        .write_all(body.as_bytes())
        .map_err(|err| AuthError::Write(path.to_path_buf(), err))?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|err| AuthError::Write(path.to_path_buf(), err))?;
    restrict(temporary.path(), 0o600)?;
    temporary
        .persist(path)
        .map_err(|err| AuthError::Write(path.to_path_buf(), err.error))?;
    sync_parent(parent).map_err(|err| AuthError::Write(path.to_path_buf(), err))
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> std::io::Result<()> {
    fs::File::open(path)?.sync_all()
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

pub(crate) fn temporary_parent(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

#[cfg(unix)]
pub(crate) fn restrict(path: &Path, mode: u32) -> Result<(), AuthError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .map_err(|err| AuthError::Write(path.to_path_buf(), err))
}

#[cfg(not(unix))]
// Windows has no Unix mode bits. ProjectDirs places the store under the
// current user's roaming profile, where new files inherit that profile's ACL;
// applying a numeric 0600 would neither describe nor improve that boundary.
pub(crate) fn restrict(_path: &Path, _mode: u32) -> Result<(), AuthError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use crate::test_support::assert_mode;

    fn assert_write_error(error: AuthError, path: &Path, kind: std::io::ErrorKind) {
        match error {
            AuthError::Write(actual_path, source) => {
                assert_eq!(actual_path, path);
                assert_eq!(source.kind(), kind);
            }
            other => panic!("expected write error, got {other:?}"),
        }
    }

    #[test]
    fn metadata_errors_are_not_treated_as_missing_directories() {
        let dir = Path::new("denied");
        let error = ensure_dir_private_with_metadata(
            dir,
            |_| Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied)),
            |_| panic!("metadata failure must not attempt directory creation"),
            |_| panic!("metadata failure must not attempt directory sync"),
        )
        .expect_err("metadata failure must remain fatal");

        assert_write_error(error, dir, std::io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn creation_errors_are_not_treated_as_races() {
        let base = tempfile::tempdir().expect("tempdir");
        let dir = base.path().join("denied");
        let error = ensure_dir_private_with(
            &dir,
            |_| Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied)),
            |_| panic!("failed directory creation must not be synced"),
        )
        .expect_err("creation failure must remain fatal");

        assert_write_error(error, &dir, std::io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn a_store_parent_must_be_a_directory() {
        let base = tempfile::tempdir().expect("tempdir");
        let file = base.path().join("not-a-directory");
        std::fs::write(&file, "data").expect("fixture file");

        let error = require_directory(&file).expect_err("file must not pass as directory");

        assert_write_error(error, &file, std::io::ErrorKind::NotADirectory);
    }

    #[test]
    fn private_directory_creation_creates_the_requested_path() {
        let base = tempfile::tempdir().expect("tempdir");
        let dir = base.path().join("private");

        create_private_dir(&dir).expect("create private directory");

        assert!(dir.is_dir());
        #[cfg(unix)]
        assert_mode(&dir, 0o700);
    }

    #[cfg(unix)]
    #[test]
    fn syncing_a_missing_directory_reports_not_found() {
        let base = tempfile::tempdir().expect("tempdir");
        let missing = base.path().join("missing");

        let error = sync_directory(&missing).expect_err("missing directory cannot be synced");

        assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
    }

    #[test]
    fn publication_syncs_the_parent_after_the_store_is_visible() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("auth.toml");
        let mut called = false;

        write_private_with(&path, "[keys]\n", |parent| {
            assert_eq!(parent, dir.path());
            assert_eq!(
                std::fs::read_to_string(&path).expect("published store"),
                "[keys]\n"
            );
            called = true;
            Ok(())
        })
        .expect("publish and sync");

        assert!(called, "the containing directory was not synced");
    }

    #[test]
    fn creating_nested_store_directories_syncs_each_new_link() {
        let base = tempfile::tempdir().expect("tempdir");
        let dir = base.path().join("first").join("second");
        let mut seen = Vec::new();

        ensure_dir_private_with(
            &dir,
            |component| {
                std::fs::create_dir(component)?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(component, std::fs::Permissions::from_mode(0o755))?;
                }
                Ok(())
            },
            |directory| {
                assert!(directory.exists(), "directory was not visible before sync");
                seen.push(directory.to_path_buf());
                Ok(())
            },
        )
        .expect("create and sync directory tree");

        assert_eq!(
            seen,
            vec![
                base.path().join("first"),
                base.path().to_path_buf(),
                dir.clone(),
                base.path().join("first"),
            ]
        );
        #[cfg(unix)]
        {
            for created in [base.path().join("first"), dir] {
                assert_mode(&created, 0o700);
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_directory_created_by_a_racing_process_is_not_claimed() {
        use std::os::unix::fs::PermissionsExt;

        let base = tempfile::tempdir().expect("tempdir");
        let dir = base.path().join("raced");

        ensure_dir_private_with(
            &dir,
            |component| {
                std::fs::create_dir(component)?;
                std::fs::set_permissions(component, std::fs::Permissions::from_mode(0o755))?;
                Err(std::io::Error::from(std::io::ErrorKind::AlreadyExists))
            },
            |_| panic!("a directory drep did not create must not be synced as its own"),
        )
        .expect("accept raced directory");

        assert_mode(&dir, 0o755);
    }
}
