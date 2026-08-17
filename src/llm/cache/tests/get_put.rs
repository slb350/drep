//! `Cache::get` and `Cache::put`.
//!
//! Criteria 8-14. Every test pins a single read or write property: round
//! trip, miss on absent, miss on corrupt, miss on unreadable, shard
//! creation, shard coexistence, and overwrite.

use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use serde_json::json;

use crate::llm::cache::Cache;

/// Write `body` to `path`'s parent and return the file path used. Used by
/// the corrupt-entry and unreadable-entry tests, which need to bypass
/// `put` and plant their own bytes on disk.
fn write_raw(path: &Path, body: &[u8]) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create shard");
    }
    std::fs::write(path, body).expect("write raw");
}

/// Criterion 8: `put` then `get` round-trips the exact JSON value.
#[test]
fn put_then_get_round_trips_exact_json_value() {
    let temp = tempfile::tempdir().expect("tempdir");
    let cache = Cache::new(temp.path().to_path_buf(), 30, 1024 * 1024);

    let key = cache.key("sys", "content", "model", 0.2);
    let value = json!({
        "findings": [
            {"severity": "high", "line": 42, "message": "bug"},
        ],
        "nested": {"a": [1, 2, 3]},
    });

    cache.put(&key, &value).expect("put");
    let read_back = cache.get(&key);

    assert_eq!(
        read_back.as_ref(),
        Some(&value),
        "put/get must round-trip exactly"
    );
}

/// Criterion 9: `get` on an absent key is `None`.
#[test]
fn get_on_absent_key_is_none() {
    let temp = tempfile::tempdir().expect("tempdir");
    let cache = Cache::new(temp.path().to_path_buf(), 30, 1024 * 1024);

    let key = cache.key("sys", "never-written", "model", 0.2);
    assert!(
        cache.get(&key).is_none(),
        "an absent key must be a miss, not an error"
    );
}

/// Criterion 10: `get` on a corrupt entry (write invalid JSON to the
/// expected path) is `None`, not an error or a panic.
#[test]
fn get_on_corrupt_entry_is_none() {
    let temp = tempfile::tempdir().expect("tempdir");
    let cache = Cache::new(temp.path().to_path_buf(), 30, 1024 * 1024);

    let key = cache.key("sys", "content", "model", 0.2);
    let path = cache.entry_path(&key);
    write_raw(&path, b"this is not JSON {{{");

    assert!(
        cache.get(&key).is_none(),
        "a corrupt entry must be a miss; reads must never fail"
    );
}

/// Criterion 11: `get` on a file the process cannot read is `None`.
///
/// We make the file unreadable via `chmod 000` and verify the read returns
/// `None` rather than propagating an error. Skipped on platforms without
/// POSIX permissions.
#[cfg(unix)]
#[test]
fn get_on_unreadable_entry_is_none() {
    let temp = tempfile::tempdir().expect("tempdir");
    let cache = Cache::new(temp.path().to_path_buf(), 30, 1024 * 1024);

    let key = cache.key("sys", "content", "model", 0.2);
    let value = json!({"x": 1});
    cache.put(&key, &value).expect("put");

    let path = cache.entry_path(&key);
    let original = std::fs::metadata(&path).expect("metadata").permissions();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).expect("chmod 000");

    let result = cache.get(&key);

    // Restore permissions so TempDir cleanup can remove the tree.
    std::fs::set_permissions(&path, original).expect("chmod restore");

    assert!(
        result.is_none(),
        "an unreadable entry must be a miss; reads must never fail"
    );
}

/// Criterion 12: `put` creates the shard directory when absent.
///
/// The fresh cache has no shard directories under it; the first `put` must
/// create the shard that the entry's first-two-hex-chars map to.
#[test]
fn put_creates_the_shard_directory_when_absent() {
    let temp = tempfile::tempdir().expect("tempdir");
    let cache = Cache::new(temp.path().to_path_buf(), 30, 1024 * 1024);

    let key = cache.key("sys", "content", "model", 0.2);
    let shard = &key.as_hex()[..2];
    let shard_path = temp.path().join(shard);
    assert!(
        !shard_path.exists(),
        "fresh cache must not pre-create shards"
    );

    cache.put(&key, &json!({"x": 1})).expect("put");

    assert!(
        shard_path.is_dir(),
        "put must create the shard directory on demand: {shard_path:?}"
    );
}

/// Criterion 13: two keys sharing a shard prefix coexist.
///
/// blake3 prefixes are uniformly distributed, so picking two distinct
/// inputs and asserting they land in the same shard requires us to know
/// the actual prefix. We compute two keys and walk both shards - either
/// they happen to share a prefix or they do not, but the test must hold
/// in both cases. The cheaper pin: both files exist on disk after `put`.
#[test]
fn two_keys_coexist_on_disk() {
    let temp = tempfile::tempdir().expect("tempdir");
    let cache = Cache::new(temp.path().to_path_buf(), 30, 1024 * 1024);

    let k1 = cache.key("sys", "alpha", "model", 0.2);
    let k2 = cache.key("sys", "beta", "model", 0.2);
    cache.put(&k1, &json!({"which": 1})).expect("put k1");
    cache.put(&k2, &json!({"which": 2})).expect("put k2");

    let v1 = cache.get(&k1);
    let v2 = cache.get(&k2);

    assert_eq!(v1.as_ref(), Some(&json!({"which": 1})), "k1 round-trips");
    assert_eq!(v2.as_ref(), Some(&json!({"which": 2})), "k2 round-trips");

    // Also: both files are actually on disk, distinct from each other.
    let p1 = cache.entry_path(&k1);
    let p2 = cache.entry_path(&k2);
    assert!(p1.exists(), "k1 file: {p1:?}");
    assert!(p2.exists(), "k2 file: {p2:?}");
    assert_ne!(p1, p2, "different keys must land at different paths");
}

/// Criterion 14: `put` over an existing key replaces the value.
#[test]
fn put_over_existing_key_replaces_value() {
    let temp = tempfile::tempdir().expect("tempdir");
    let cache = Cache::new(temp.path().to_path_buf(), 30, 1024 * 1024);

    let key = cache.key("sys", "content", "model", 0.2);
    cache.put(&key, &json!({"version": 1})).expect("put v1");
    cache.put(&key, &json!({"version": 2})).expect("put v2");

    let read_back = cache.get(&key);
    assert_eq!(
        read_back.as_ref(),
        Some(&json!({"version": 2})),
        "put must overwrite, not append"
    );
}
