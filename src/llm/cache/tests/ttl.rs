//! TTL behaviour.
//!
//! Criteria 15-17. Three pins: an expired entry is a miss, a fresh entry
//! is a hit, and reading an expired entry removes it from disk.

use std::path::Path;
use std::time::SystemTime;

use serde_json::json;

use crate::llm::cache::Cache;

/// Set the file at `path`'s mtime to `mtime`. Used by the expiry tests to
/// plant an entry that already looks older than the TTL.
fn set_mtime(path: &Path, mtime: SystemTime) {
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .expect("open for mtime set");
    file.set_modified(mtime).expect("set_modified");
}

/// Criterion 15: an entry older than the TTL is a miss.
///
/// We plant an entry, then push its mtime into the past far enough that
/// `age > ttl`. The next `get` must return `None`.
#[test]
fn entry_older_than_ttl_is_a_miss() {
    let temp = tempfile::tempdir().expect("tempdir");
    let cache = Cache::new(temp.path().to_path_buf(), 30, 1024 * 1024);

    let key = cache.key(
        "sys",
        "content",
        "http://endpoint/v1",
        "model",
        "openai",
        Some(0.2),
    );
    cache.put(&key, &json!({"x": 1})).expect("put");
    let path = cache.entry_path(&key);

    // Backdate the file by 60 days; TTL is 30 days, so age > ttl.
    let ancient = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(60 * 86_400);
    set_mtime(&path, ancient);

    assert!(
        cache.get(&key).is_none(),
        "an entry older than TTL must be a miss"
    );
}

/// Criterion 16: an entry within the TTL is a hit.
///
/// A fresh `put` writes the file with `now` as mtime, and the default
/// 30-day TTL means a read immediately after the write is well within
/// bounds. The full value must round-trip, not just the key.
#[test]
fn entry_within_ttl_is_a_hit() {
    let temp = tempfile::tempdir().expect("tempdir");
    let cache = Cache::new(temp.path().to_path_buf(), 30, 1024 * 1024);

    let key = cache.key(
        "sys",
        "content",
        "http://endpoint/v1",
        "model",
        "openai",
        Some(0.2),
    );
    let value = json!({"answer": 42});
    cache.put(&key, &value).expect("put");

    assert_eq!(
        cache.get(&key).as_ref(),
        Some(&value),
        "a fresh entry must hit"
    );
}

/// Criterion 17: reading an expired entry removes it from disk.
///
/// The opportunistic removal in `get` keeps the tree from filling with
/// dead weight between explicit `evict_if_needed` calls.
#[test]
fn reading_expired_entry_removes_it_from_disk() {
    let temp = tempfile::tempdir().expect("tempdir");
    let cache = Cache::new(temp.path().to_path_buf(), 30, 1024 * 1024);

    let key = cache.key(
        "sys",
        "content",
        "http://endpoint/v1",
        "model",
        "openai",
        Some(0.2),
    );
    cache.put(&key, &json!({"x": 1})).expect("put");
    let path = cache.entry_path(&key);

    let ancient = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(60 * 86_400);
    set_mtime(&path, ancient);

    let _ = cache.get(&key);

    assert!(
        !path.exists(),
        "reading an expired entry must remove the file from disk: {path:?}"
    );
}

/// Pins the `>` (strict) comparison at the age == ttl boundary.
///
/// Without a real clock mock, `age == ttl` cannot be reached from a
/// past mtime: any `set_modified` happens strictly before the read's
/// `SystemTime::now()`, so age is always strictly greater than zero and
/// strictly less than `now - mtime`. We use the future-mtime trick
/// instead: `duration_since` returns `Err` for mtime > now, and
/// `get` falls back to `Duration::ZERO`. With `ttl_days = 0` (TTL =
/// `Duration::ZERO`), age == ttl exactly.
///
/// Original code: `age > ttl` -> `0 > 0` is false -> hit.
/// Mutant (`>=`): `0 >= 0` is true -> miss.
///
/// This is the only configuration that distinguishes `>` from `>=`
/// without a time-mocking dependency, and it catches the cargo-mutants
/// substitution before the rest of the TTL tests (which differ only by
/// a comfortable margin) can.
#[test]
fn age_equal_to_ttl_is_a_hit_not_a_miss() {
    let temp = tempfile::tempdir().expect("tempdir");
    // TTL = Duration::ZERO; the boundary is exactly age 0.
    let cache = Cache::new(temp.path().to_path_buf(), 0, 1024 * 1024);

    let key = cache.key(
        "sys",
        "content",
        "http://endpoint/v1",
        "model",
        "openai",
        Some(0.2),
    );
    let value = json!({"x": 1});
    cache.put(&key, &value).expect("put");

    // Plant a future mtime so `duration_since` falls back to ZERO.
    let path = cache.entry_path(&key);
    let future = SystemTime::now() + std::time::Duration::from_secs(3600);
    set_mtime(&path, future);

    assert_eq!(
        cache.get(&key).as_ref(),
        Some(&value),
        "age == ttl must be a hit under strict `>` semantics"
    );
}
