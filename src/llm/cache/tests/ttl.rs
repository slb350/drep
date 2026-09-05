//! TTL behaviour.
//!
//! Expired entries miss and are removed; the exact TTL boundary remains a hit.
//! Fresh-entry hits are covered by the put/get roundtrip tests.

use std::time::SystemTime;

use serde_json::json;

use crate::llm::cache::Cache;
use crate::test_support::set_mtime;

/// Reading an expired entry returns a miss and removes it from disk.
#[test]
fn an_expired_entry_is_a_miss_and_is_removed() {
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

    // An mtime near the Unix epoch is well beyond the 30-day TTL.
    let ancient = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(60 * 86_400);
    set_mtime(&path, ancient);

    assert!(
        cache.get(&key).is_none(),
        "an entry older than TTL must be a miss"
    );
    assert!(
        !path.exists(),
        "reading an expired entry must remove the file from disk: {path:?}"
    );
}

/// A future mtime clamps age to zero. With a zero TTL, this pins the exact
/// boundary: age == ttl remains a hit; only age > ttl expires.
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
