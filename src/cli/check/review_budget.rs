//! Atomic, worktree-local accounting for semantic remediation rounds.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};

/// A killed review may leave a pending slot behind. A week is longer than any
/// supported provider timeout multiplied across a realistic review, while
/// still making a crashed process recover without manual state surgery.
pub(super) const PENDING_LEASE_SECS: u64 = 7 * 24 * 60 * 60;

const DIRECTORY: &str = "review-cycles-v1";
const SLOT_PREFIX: &str = "round-";
const SLOT_SUFFIX: &str = ".state";

// `flock` is process-associated on some Unix targets, so two threads in this
// process may both appear to hold the same advisory file lock. The mutex covers
// that case; the file lock covers independent drep processes.
static PROCESS_LOCK: Mutex<()> = Mutex::new(());
static TOKEN_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug)]
pub(super) struct Budget {
    cycles_root: PathBuf,
    lock_path: PathBuf,
    directory: PathBuf,
    limit: u32,
    now: u64,
}

#[derive(Debug)]
pub(super) enum Claim {
    Reserved(Reservation),
    LimitReached { completed: u32, limit: u32 },
}

#[derive(Debug)]
pub(super) struct Reservation {
    lock_path: PathBuf,
    path: PathBuf,
    round: u32,
    pending: String,
    committed: bool,
}

impl Budget {
    pub(super) async fn for_repo(root: &Path, limit: u32) -> Result<Self> {
        let git_dir = crate::diff::git_path(root, &["rev-parse", "--git-dir"])
            .await
            .context("could not locate Git metadata for review-round accounting")?;
        let identity = crate::diff::git_query(root, &["symbolic-ref", "--quiet", "HEAD"])
            .await
            .context("could not identify the Git branch for review-round accounting")?
            .unwrap_or_else(|| "detached-head".to_owned());
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("system clock is before the Unix epoch")?
            .as_secs();
        Ok(Self::at(&git_dir.join("drep"), identity.trim(), limit, now))
    }

    pub(super) fn at(root: &Path, identity: &str, limit: u32, now: u64) -> Self {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"drep-review-cycle-v1\0");
        hasher.update(identity.as_bytes());
        let key = hasher.finalize().to_hex();
        let cycles_root = root.join(DIRECTORY);
        Self {
            directory: cycles_root.join(key.as_str()),
            cycles_root,
            lock_path: root.join(format!("{DIRECTORY}.lock")),
            limit,
            now,
        }
    }

    pub(super) fn claim(&self) -> Result<Claim> {
        let _process_lock = process_lock();
        let _file_lock = lock_at(&self.lock_path)?;
        fs::create_dir_all(&self.directory)
            .with_context(|| format!("could not create {}", self.directory.display()))?;

        for round in 1..=self.limit {
            let path = self.slot_path(round);
            if self.reap_if_stale(&path)? {
                return Ok(Claim::Reserved(self.create_pending(path, round)?));
            }
        }

        Ok(Claim::LimitReached {
            completed: self.committed_count()?,
            limit: self.limit,
        })
    }

    pub(super) fn reset(&self) -> Result<bool> {
        // Avoid creating review state merely to prove that none exists. A
        // claimant racing this check owns a new cycle and should survive this
        // clean run, so returning early is safe in either ordering.
        if !self.directory.exists() {
            return Ok(false);
        }
        let _process_lock = process_lock();
        let _file_lock = lock_at(&self.lock_path)?;
        let entries = match fs::read_dir(&self.directory) {
            Ok(entries) => entries,
            Err(err) => {
                return Err(err)
                    .with_context(|| format!("could not read {}", self.directory.display()));
            }
        };
        let mut removed = false;
        for entry in entries {
            let entry = entry
                .with_context(|| format!("could not enumerate {}", self.directory.display()))?;
            if slot_number(&entry.file_name()).is_some()
                && fs::read_to_string(entry.path()).is_ok_and(|raw| raw == "committed\n")
            {
                fs::remove_file(entry.path()).with_context(|| {
                    format!("could not reset review slot {}", entry.path().display())
                })?;
                removed = true;
            }
        }
        if fs::read_dir(&self.directory)
            .with_context(|| format!("could not read {}", self.directory.display()))?
            .next()
            .is_none()
        {
            fs::remove_dir(&self.directory)
                .with_context(|| format!("could not remove {}", self.directory.display()))?;
        }
        if fs::read_dir(&self.cycles_root).is_ok_and(|mut entries| entries.next().is_none()) {
            fs::remove_dir(&self.cycles_root)
                .with_context(|| format!("could not remove {}", self.cycles_root.display()))?;
        }
        Ok(removed)
    }

    #[cfg(test)]
    pub(super) fn directory(&self) -> &Path {
        &self.directory
    }

    fn slot_path(&self, round: u32) -> PathBuf {
        self.directory
            .join(format!("{SLOT_PREFIX}{round}{SLOT_SUFFIX}"))
    }

    fn committed_count(&self) -> Result<u32> {
        let mut completed = 0;
        for round in 1..=self.limit {
            match fs::read_to_string(self.slot_path(round)) {
                Ok(raw) if raw == "committed\n" => completed += 1,
                Ok(_) => {}
                Err(err) => {
                    return Err(err)
                        .with_context(|| format!("could not read review slot for round {round}"));
                }
            }
        }
        Ok(completed)
    }

    /// True means the caller should attempt `create_new`; false means a live
    /// or deliberately fail-closed slot still occupies this round.
    fn reap_if_stale(&self, path: &Path) -> Result<bool> {
        let stale = match fs::read_to_string(path) {
            Ok(raw) if raw == "committed\n" => false,
            Ok(raw) => raw
                .strip_prefix("pending\n")
                .and_then(|s| s.lines().next())
                .and_then(|started| started.parse().ok())
                .map_or_else(
                    || self.is_stale_by_modified_time(path),
                    |started| lease_expired(self.now, started),
                ),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(true),
            // An unreadable or incomplete slot remains fail-closed for a full
            // lease, then becomes recoverable. This covers a process killed
            // after `create_new` but before its pending timestamp was synced.
            Err(_) => self.is_stale_by_modified_time(path),
        };
        if !stale {
            return Ok(false);
        }
        match fs::remove_file(path) {
            Ok(()) => Ok(true),
            Err(err) => Err(err)
                .with_context(|| format!("could not reclaim stale review slot {}", path.display())),
        }
    }

    fn is_stale_by_modified_time(&self, path: &Path) -> bool {
        fs::metadata(path)
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
            .is_some_and(|modified| lease_expired(self.now, modified.as_secs()))
    }

    fn create_pending(&self, path: PathBuf, round: u32) -> Result<Reservation> {
        let mut file = match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => file,
            Err(err) => {
                return Err(err)
                    .with_context(|| format!("could not reserve review slot {}", path.display()));
            }
        };
        let pending = format!("pending\n{}\n{}\n", self.now, reservation_token());
        if let Err(err) = file
            .write_all(pending.as_bytes())
            .and_then(|()| file.sync_all())
        {
            let _ = fs::remove_file(&path);
            return Err(err)
                .with_context(|| format!("could not write review slot {}", path.display()));
        }
        Ok(Reservation {
            lock_path: self.lock_path.clone(),
            path,
            round,
            pending,
            committed: false,
        })
    }
}

fn lock_at(path: &Path) -> Result<fs::File> {
    let parent = path
        .parent()
        .context("review-budget lock has no parent directory")?;
    fs::create_dir_all(parent).with_context(|| format!("could not create {}", parent.display()))?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .with_context(|| format!("could not open review-budget lock {}", path.display()))?;
    fs2::FileExt::lock_exclusive(&file)
        .with_context(|| format!("could not lock review budget {}", path.display()))?;
    Ok(file)
}

fn reservation_token() -> String {
    let sequence = TOKEN_COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{:x}-{nanos:x}-{sequence:x}", std::process::id())
}

fn process_lock() -> MutexGuard<'static, ()> {
    PROCESS_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub(super) fn lease_expired(now: u64, started: u64) -> bool {
    now.saturating_sub(started) > PENDING_LEASE_SECS
}

pub(super) fn is_authoritative(args: &super::CheckArgs) -> bool {
    args.staged || is_completion_scope(args)
}

pub(super) fn is_completion_scope(args: &super::CheckArgs) -> bool {
    args.diff.is_some() || args.pre_commit_push || (args.push_gate && args.paths.is_empty())
}

impl Reservation {
    pub(super) fn round(&self) -> u32 {
        self.round
    }

    pub(super) fn commit(mut self) -> Result<()> {
        let _process_lock = process_lock();
        let _file_lock = lock_at(&self.lock_path)?;
        if fs::read_to_string(&self.path).ok().as_deref() != Some(self.pending.as_str()) {
            bail!(
                "review slot {} is no longer owned by this review",
                self.path.display()
            );
        }
        let mut file = OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&self.path)
            .with_context(|| format!("could not reopen review slot {}", self.path.display()))?;
        file.write_all(b"committed\n")
            .and_then(|()| file.sync_all())
            .with_context(|| format!("could not commit review slot {}", self.path.display()))?;
        self.committed = true;
        Ok(())
    }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        let _process_lock = process_lock();
        if let Ok(_file_lock) = lock_at(&self.lock_path)
            && fs::read_to_string(&self.path).ok().as_deref() == Some(self.pending.as_str())
        {
            let _ = fs::remove_file(&self.path);
        }
    }
}

fn slot_number(name: &std::ffi::OsStr) -> Option<u32> {
    name.to_str()?
        .strip_prefix(SLOT_PREFIX)?
        .strip_suffix(SLOT_SUFFIX)?
        .parse()
        .ok()
}
