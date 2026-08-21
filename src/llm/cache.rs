//! Content-addressed cache for LLM responses.
//!
//! Two design choices diverge from the Python `drep/llm/cache.py` and the
//! criterion is "the cache should hit on the call paths the gate actually
//! takes", not "the cache should mirror what 1.x did":
//!
//! - **The key is content-only, never commit-aware.** The Python mixed the
//!   commit SHA into the key so a new commit invalidated everything. Hashing
//!   the content already invalidates precisely when the content changes; the
//!   SHA was forcing a miss on every unchanged file at every new commit, which
//!   is the exact case the cache exists to serve. On a pre-commit gate that is
//!   the common path.
//! - **One file per entry, the value is the JSON.** No sidecar metadata file.
//!   The key is the filename, so nothing has to be re-validated on read except
//!   age. If the entry is corrupt, unreadable, or expired, the read returns
//!   `None` and the caller re-queries and overwrites. A cache is an
//!   optimisation; it must not be able to take the gate down.
//!
//! ## Storage layout
//!
//! `root/<first two hex chars>/<full hex>.json`. Sharding keeps directories
//! from growing to tens of thousands of entries (a flat `root/<hex>.json`
//! layout would do the same for `readdir`, but `ls`-ing the cache to debug
//! it would take seconds).
//!
//! ## Key composition
//!
//! `blake3` over the six inputs, each length-prefixed with an 8-byte
//! big-endian length. Length prefixing rules out the
//! `key("ab", "c", ...)` vs `key("a", "bc", ...)` collision that a separator
//! byte cannot guarantee once `content` or `system_prompt` is allowed to
//! contain that byte. `temperature` is formatted with a fixed number of
//! decimals so `0.2` and `0.20` hash to the same key (they are the same
//! value).
//!
//! **The backend identity is part of the key, not just the model.** A model name is
//! not a globally unique identity: the canonical failover pair is one open
//! model served from a local runtime and from a cloud provider, which name it
//! identically. Keyed on the model alone, the fallback's answer lands where the
//! head would look for its own - so a later run with the head restored is
//! served a response it never produced, which is the exact defect the
//! per-provider key exists to prevent.
//!
//! ## Defaults
//!
//! 30 days TTL, 256 MiB max bytes.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde_json::Value;
use thiserror::Error;

/// A cache key: the blake3 hex digest of the six key inputs.
///
/// The inner `String` is exactly 64 lower-case ASCII hex characters because
/// `blake3::Hash::to_hex` always emits 64 hex chars. Tests rely on this to
/// compute the on-disk path by hand.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CacheKey(String);

impl CacheKey {
    /// The hex digest. Test-only path construction uses this.
    pub(crate) fn as_hex(&self) -> &str {
        &self.0
    }
}

/// What can go wrong at write time.
///
/// Reads are infallible by design: a corrupt or missing entry is a miss, not
/// an error, so `get` returns `Option<Value>`. The caller treats write
/// failures as non-fatal (log and continue); only the path to disk, the
/// serialisation, the directory creation, and eviction need error variants.
#[derive(Debug, Error)]
pub enum CacheError {
    /// The shard directory could not be created on `put`.
    #[error("could not create cache shard {0}: {1}")]
    CreateShard(PathBuf, std::io::Error),
    /// Serialising the value to JSON failed. Should be unreachable for
    /// values produced by `serde_json` itself, but the type permits any
    /// `Value`, so the error path exists.
    #[error("could not serialise cache entry {0}: {1}")]
    Serialize(String, serde_json::Error),
    /// Writing the JSON file failed (disk full, permissions, race with
    /// concurrent eviction).
    #[error("could not write cache entry {0}: {1}")]
    Write(PathBuf, std::io::Error),
    /// Reading the cache directory during eviction failed.
    #[error("could not read cache directory: {0}")]
    Walk(std::io::Error),
    /// Stat-ing an entry during eviction failed.
    #[error("could not stat cache entry {0}: {1}")]
    Stat(PathBuf, std::io::Error),
    /// Removing an entry during eviction failed.
    #[error("could not remove cache entry {0}: {1}")]
    Remove(PathBuf, std::io::Error),
}

/// The cache: a directory tree of one JSON file per entry, keyed by blake3
/// digest of the six prompt and backend inputs.
///
/// Built once per process and shared across the analyzer; every method takes
/// `&self` so `Cache` can live behind an `Arc` if a future caller needs it.
#[derive(Debug, Clone)]
pub struct Cache {
    root: PathBuf,
    ttl: Duration,
    max_bytes: u64,
}

impl Cache {
    /// Build a cache rooted at `root`. Creates `root` if absent; a creation
    /// failure is swallowed (the next `put` will surface it).
    pub fn new(root: PathBuf, ttl_days: u64, max_bytes: u64) -> Self {
        let _ = std::fs::create_dir_all(&root);
        Self {
            root,
            ttl: Duration::from_secs(ttl_days.saturating_mul(86_400)),
            max_bytes,
        }
    }

    /// The conventional location: `directories::ProjectDirs::cache_dir()`,
    /// falling back to `.drep-cache` in the cwd when the platform has no
    /// cache directory. Returned as a relative path on the fallback branch
    /// so the caller can resolve it against whatever cwd they choose.
    pub fn default_root() -> PathBuf {
        if let Some(dirs) = directories::ProjectDirs::from("dev", "slb350", "drep") {
            return dirs.cache_dir().to_path_buf();
        }
        PathBuf::from(".drep-cache")
    }

    /// Compute the key for
    /// `(system_prompt, content, backend, model, request_shape, temperature)`.
    ///
    /// Deliberately does NOT consult `self`: the key is content-only, so two
    /// `Cache` instances at different roots produce the same key for the
    /// same inputs. That is what criterion 7 asserts and what makes the
    /// cache portable across CI runs.
    ///
    /// `request_shape` is in the key because one endpoint can serve the same model
    /// over both wire formats - `api.minimax.io` publishes `/v1` and
    /// `/anthropic/v1` for `MiniMax-M3` - and the two are different requests
    /// with different reasoning handling. Keying without it files one
    /// protocol's answer where the other looks for its own, which is the same
    /// defect that put `endpoint` in the key.
    pub fn key(
        &self,
        system_prompt: &str,
        content: &str,
        backend: &str,
        model: &str,
        request_shape: &str,
        temperature: Option<f32>,
    ) -> CacheKey {
        let mut hasher = blake3::Hasher::new();
        write_field(&mut hasher, system_prompt.as_bytes());
        write_field(&mut hasher, content.as_bytes());
        write_field(&mut hasher, backend.as_bytes());
        write_field(&mut hasher, model.as_bytes());
        write_field(&mut hasher, request_shape.as_bytes());
        // `{:?}` on an `f32` is the *shortest string that round-trips*, so two
        // distinct `f32`s always render differently and `0.2` and `0.20` - the
        // same value - render the same. That is exactly the property a key
        // needs.
        //
        // This replaced `{:.6}`, whose comment claimed six decimal places were
        // finer than `f32`'s resolution. They are not: `f32` has ~7 significant
        // digits, not 7 decimal places, so near 1.0 its ulp is ~1.2e-7 while
        // six decimals steps by 1e-6 - coarser, and able to collapse two
        // genuinely different temperatures onto one key.
        //
        // An unset temperature is a *different request* from any set one - the
        // field is absent and the server picks - so it gets a sentinel that no
        // formatted float can collide with, rather than being folded onto some
        // stand-in value.
        let temp_str = match temperature {
            Some(value) => format!("{value:?}"),
            None => "unset".to_string(),
        };
        write_field(&mut hasher, temp_str.as_bytes());
        CacheKey(hasher.finalize().to_hex().to_string())
    }

    /// Read the entry at `key`, or `None` if absent, expired, unreadable, or
    /// unparseable. Reads never fail.
    ///
    /// An expired entry is removed opportunistically; a corrupt or
    /// unreadable entry is left in place so the next read has another chance
    /// (transient I/O is more likely than persistent corruption, and a
    /// half-deleted cache is worse than a slightly redundant one).
    pub fn get(&self, key: &CacheKey) -> Option<Value> {
        let path = self.entry_path(key);
        let meta = std::fs::metadata(&path).ok()?;
        let mtime = meta.modified().ok()?;
        // A future mtime (clock skew, manual planting) makes
        // `duration_since` return `Err`. Treat that as age zero rather
        // than propagating the error: the entry is well within TTL,
        // and the alternative would silently turn every clock-skewed
        // cache into a miss.
        let age = SystemTime::now()
            .duration_since(mtime)
            .unwrap_or(Duration::ZERO);
        if age > self.ttl {
            // Expired. Removing is best-effort: a stale entry is no worse
            // than a missing one, but failing to remove is not worth a Result.
            let _ = std::fs::remove_file(&path);
            return None;
        }
        let bytes = std::fs::read(&path).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    /// Write `value` to disk under `key`.
    ///
    /// Creates the shard directory on demand so the very first `put` into a
    /// fresh cache does not need a separate bootstrap step.
    pub fn put(&self, key: &CacheKey, value: &Value) -> Result<(), CacheError> {
        let path = self.entry_path(key);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| CacheError::CreateShard(parent.to_path_buf(), e))?;
        }
        let bytes = serde_json::to_vec(value)
            .map_err(|e| CacheError::Serialize(key.as_hex().to_owned(), e))?;
        std::fs::write(&path, &bytes).map_err(|e| CacheError::Write(path.clone(), e))?;
        Ok(())
    }

    /// Walk the tree, evicting the oldest entries (by mtime) until the
    /// total size is at or under `max_bytes`. Returns the number of bytes
    /// freed; `0` when no eviction was needed.
    pub fn evict_if_needed(&self) -> Result<u64, CacheError> {
        let mut entries = self.collect_entries()?;
        let total: u64 = entries.iter().map(|e| e.size).sum();
        if total <= self.max_bytes {
            return Ok(0);
        }
        entries.sort_by_key(|e| e.mtime);
        let mut current = total;
        let mut freed = 0u64;
        for entry in entries {
            if current <= self.max_bytes {
                break;
            }
            std::fs::remove_file(&entry.path)
                .map_err(|e| CacheError::Remove(entry.path.clone(), e))?;
            current = current.saturating_sub(entry.size);
            freed = freed.saturating_add(entry.size);
        }
        Ok(freed)
    }

    /// Compute the on-disk path for `key`.
    ///
    /// `pub(crate)` so the test suite can construct the same path to plant
    /// an expired mtime or to write a corrupt body for the miss-on-bad-data
    /// criterion.
    pub(crate) fn entry_path(&self, key: &CacheKey) -> PathBuf {
        let hex = key.as_hex();
        // blake3::Hash::to_hex always produces 64 ASCII hex chars, so the
        // `..2` slice is safe by construction.
        let shard = &hex[..2];
        self.root.join(shard).join(format!("{hex}.json"))
    }

    /// Walk every entry under `root` and return `(path, mtime, size)`.
    ///
    /// Skips the shard-directories' own metadata (no file under them, no
    /// entry). Entries we cannot stat are skipped silently: a transient
    /// stat failure should not abort eviction when the tree still has
    /// deletable members.
    fn collect_entries(&self) -> Result<Vec<CacheEntry>, CacheError> {
        let mut out = Vec::new();
        let shards = std::fs::read_dir(&self.root).map_err(CacheError::Walk)?;
        for shard in shards {
            let shard = shard.map_err(CacheError::Walk)?;
            // Only descend into the two-hex-char directories. Anything else -
            // a stray file the user dropped in the cache root, or a directory
            // that is not one of ours - is ignored, so nothing outside the
            // layout this module writes can be evicted.
            //
            // The name check is load-bearing, not decorative. `is_dir()` alone
            // was what the code did while this comment claimed otherwise, which
            // meant `evict_if_needed` - the one destructive path here - would
            // happily delete files out of *any* directory someone had placed
            // under the cache root.
            let file_type = match shard.file_type() {
                Ok(ft) => ft,
                Err(_) => continue,
            };
            if !file_type.is_dir() {
                continue;
            }
            if !is_shard_name(&shard.file_name()) {
                continue;
            }
            let shard_path = shard.path();
            let shard_name = shard.file_name();
            let entries = match std::fs::read_dir(&shard_path) {
                Ok(e) => e,
                Err(_) => continue,
            };
            for entry in entries {
                let entry = match entry {
                    Ok(e) => e,
                    Err(_) => continue,
                };
                if !is_entry_name(&shard_name, &entry.file_name()) {
                    continue;
                }
                let path = entry.path();
                let file_type = match entry.file_type() {
                    Ok(file_type) => file_type,
                    Err(_) => continue,
                };
                if !file_type.is_file() {
                    continue;
                }
                let meta = match entry.metadata() {
                    Ok(m) => m,
                    Err(_) => continue,
                };
                if !meta.is_file() {
                    continue;
                }
                // Falling back to UNIX_EPOCH on a failed `modified()` means
                // the entry sorts first in eviction - i.e., it would be
                // evicted first. That is the safe direction: a cache that
                // evicts an unreadable entry loses a redundant file, not
                // correctness.
                let mtime = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
                let size = meta.len();
                out.push(CacheEntry { path, mtime, size });
            }
        }
        Ok(out)
    }
}

impl Cache {
    /// The cache root directory. `pub(crate)` for tests that need to assert
    /// sharding / shard creation.
    #[allow(dead_code)]
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }
}

/// One entry on disk, pre-computed for the eviction walk.
struct CacheEntry {
    path: PathBuf,
    mtime: SystemTime,
    size: u64,
}

/// Whether `name` is one of this module's shard directories.
///
/// Exactly two lower-case hex characters, which is what [`Cache::entry_path`]
/// produces from a blake3 digest. Written against the same alphabet rather than
/// a looser "two characters" check, so a directory named `ab` is a shard and
/// one named `zz` or `AB` is not.
fn is_shard_name(name: &std::ffi::OsStr) -> bool {
    name.to_str().is_some_and(|name| is_lower_hex(name, 2))
}

/// Whether `name` has the exact form written by [`Cache::entry_path`] for
/// `shard`: 64 lower-case hexadecimal digest characters plus `.json`, with
/// the digest beginning with the parent shard name.
fn is_entry_name(shard: &std::ffi::OsStr, name: &std::ffi::OsStr) -> bool {
    let (Some(shard), Some(name)) = (shard.to_str(), name.to_str()) else {
        return false;
    };
    let Some(digest) = name.strip_suffix(".json") else {
        return false;
    };
    digest.starts_with(shard) && is_lower_hex(digest, 64)
}

fn is_lower_hex(value: &str, expected_len: usize) -> bool {
    value.len() == expected_len
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Write a length-prefixed field to the hasher.
///
/// Length-prefixing rules out boundary collisions (`("ab","c")` vs
/// `("a","bc")`) without depending on any byte that cannot appear in the
/// payload. The length is a fixed 8-byte big-endian `u64`, so a single
/// field can be at most `u64::MAX` bytes long - far longer than any real
/// prompt.
fn write_field(hasher: &mut blake3::Hasher, bytes: &[u8]) {
    let len = u64::try_from(bytes.len()).expect("prompt field longer than u64::MAX bytes");
    hasher.update(&len.to_be_bytes());
    hasher.update(bytes);
}

#[cfg(test)]
mod tests;
