//! Eviction behaviour.
//!
//! Criteria 18-20. Three pins: `evict_if_needed` reduces the tree under
//! the limit and reports bytes freed; the oldest entry is removed first;
//! a tree already under the limit is a no-op returning 0.

use std::time::SystemTime;

use serde_json::json;

use crate::llm::cache::{Cache, CacheError};
use crate::test_support::set_mtime;

/// An absent cache is empty; an invalid root is a walk failure.
#[test]
fn eviction_distinguishes_an_absent_cache_from_an_invalid_root() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("absent-cache");
    let cache = Cache::new(root.clone(), 30, 60);

    assert_eq!(cache.evict_if_needed().expect("missing means empty"), 0);
    assert!(
        !root.exists(),
        "eviction must not create an empty cache root"
    );

    std::fs::write(&root, b"not a directory").expect("invalid cache root");
    assert!(matches!(
        cache.evict_if_needed(),
        Err(CacheError::Walk(err)) if err.kind() == std::io::ErrorKind::NotADirectory
    ));
}

/// Criterion 18: with `max_bytes` smaller than the tree,
/// `evict_if_needed` removes entries until under the limit and reports
/// the bytes freed.
#[test]
fn evict_if_needed_removes_until_under_limit_and_reports_freed() {
    let temp = tempfile::tempdir().expect("tempdir");
    // Limit set deliberately below the tree size so eviction has work.
    // Two ~50-byte payloads exceed the 60-byte cap, forcing at least one
    // entry out.
    let cache = Cache::new(temp.path().to_path_buf(), 30, 60);

    let k1 = cache.key(
        "sys",
        "alpha",
        "http://endpoint/v1",
        "model",
        "openai",
        Some(0.2),
    );
    let k2 = cache.key(
        "sys",
        "beta",
        "http://endpoint/v1",
        "model",
        "openai",
        Some(0.2),
    );
    cache
        .put(
            &k1,
            &json!({"payload": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}),
        )
        .expect("put k1");
    cache
        .put(
            &k2,
            &json!({"payload": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"}),
        )
        .expect("put k2");

    let freed = cache.evict_if_needed().expect("eviction");
    let remaining = crate::test_support::two_level_tree_size(temp.path());

    assert!(
        remaining <= 60,
        "tree must be under max_bytes after eviction, was {remaining}"
    );
    assert!(
        freed > 0,
        "freed must be positive when eviction ran, was {freed}"
    );
}

/// Criterion 19: eviction removes the **oldest** entry first, not an
/// arbitrary one. We write two entries with deliberately distinct mtimes
/// and assert the newer one survives.
#[test]
fn eviction_removes_oldest_entry_first() {
    let temp = tempfile::tempdir().expect("tempdir");
    // Two ~50-byte payloads exceed the 60-byte cap, so eviction has
    // exactly one entry to remove. With two files of equal size and a
    // cap that fits one but not both, the older must go.
    let cache = Cache::new(temp.path().to_path_buf(), 30, 60);

    let k_old = cache.key(
        "sys",
        "older",
        "http://endpoint/v1",
        "model",
        "openai",
        Some(0.2),
    );
    let k_new = cache.key(
        "sys",
        "newer",
        "http://endpoint/v1",
        "model",
        "openai",
        Some(0.2),
    );

    cache
        .put(
            &k_old,
            &json!({"payload": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}),
        )
        .expect("put old");
    cache
        .put(
            &k_new,
            &json!({"payload": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"}),
        )
        .expect("put new");

    let old_path = cache.entry_path(&k_old);
    let new_path = cache.entry_path(&k_new);

    // Push `k_old`'s mtime into the past; `k_new`'s stays at "now".
    set_mtime(
        &old_path,
        SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(86_400),
    );

    let freed = cache.evict_if_needed().expect("eviction");
    assert!(freed > 0, "eviction must run, freed {freed}");

    assert!(
        !old_path.exists(),
        "the older entry must be evicted: {old_path:?}"
    );
    assert!(
        new_path.exists(),
        "the newer entry must survive eviction: {new_path:?}"
    );
}

/// Criterion 20: `evict_if_needed` on a tree already under the limit
/// removes nothing and returns 0.
#[test]
fn evict_if_needed_is_a_no_op_when_already_under_limit() {
    let temp = tempfile::tempdir().expect("tempdir");
    // Plenty of headroom; entries are small.
    let cache = Cache::new(temp.path().to_path_buf(), 30, 10 * 1024 * 1024);

    let k = cache.key(
        "sys",
        "content",
        "http://endpoint/v1",
        "model",
        "openai",
        Some(0.2),
    );
    cache.put(&k, &json!({"x": 1})).expect("put");

    let freed = cache.evict_if_needed().expect("eviction");
    assert_eq!(freed, 0, "no eviction needed, freed must be 0");
    assert!(
        cache.entry_path(&k).exists(),
        "the entry must survive a no-op eviction"
    );
}

/// Valid-looking entries in directories outside the two-character shard
/// layout must survive eviction, along with stray files in the root.
#[test]
fn eviction_ignores_directories_that_are_not_shards() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().to_path_buf();
    // A tiny ceiling, so eviction definitely runs.
    let cache = Cache::new(root.clone(), 30, 1);

    // A real entry, so the cache has something of its own to evict.
    let key = cache.key(
        "sys",
        "content",
        "http://e/v1",
        "model",
        "openai",
        Some(0.2),
    );
    cache.put(&key, &serde_json::json!({"a": 1})).expect("put");

    let stray_file = root.join("notes.txt");
    std::fs::write(&stray_file, b"do not delete me").expect("stray file");
    let foreign_files = ["a", "aaa"].map(|name| {
        let directory = root.join(name);
        std::fs::create_dir_all(&directory).expect("foreign directory");
        let file = directory.join(format!("{}.json", "a".repeat(64)));
        std::fs::write(&file, vec![0u8; 4096]).expect("foreign file");
        file
    });

    cache.evict_if_needed().expect("eviction runs");

    for file in foreign_files {
        assert!(file.exists(), "eviction removed a foreign entry: {file:?}");
    }
    assert!(
        stray_file.exists(),
        "a stray file in the root is not an entry"
    );
}

/// A valid shard directory can still contain a file the cache did not create.
/// Eviction recognizes the complete `<digest>.json` layout, not merely the
/// parent directory name.
#[test]
fn eviction_ignores_foreign_files_inside_a_valid_shard() {
    let temp = tempfile::tempdir().expect("tempdir");
    let cache = Cache::new(temp.path().to_path_buf(), 30, 1);
    let key = cache.key("sys", "content", "http://e/v1", "model", "openai", None);
    cache.put(&key, &json!({"a": 1})).expect("cache entry");

    let entry_path = cache.entry_path(&key);
    let shard = entry_path.parent().expect("shard");
    let foreign = shard.join("notes.json");
    std::fs::write(&foreign, vec![0_u8; 4096]).expect("foreign file");

    cache.evict_if_needed().expect("eviction");

    assert!(
        foreign.exists(),
        "eviction removed a file it did not create"
    );
}

#[test]
fn eviction_requires_the_digest_to_be_lower_hex_and_match_its_shard() {
    let temp = tempfile::tempdir().expect("tempdir");
    let cache = Cache::new(temp.path().to_path_buf(), 30, 0);

    let wrong_shard = temp.path().join("ff");
    std::fs::create_dir_all(&wrong_shard).expect("wrong shard");
    let wrong_shard_entry = wrong_shard.join(format!("{}.json", "a".repeat(64)));
    std::fs::write(&wrong_shard_entry, b"x").expect("foreign file");

    let invalid_hex_shard = temp.path().join("ab");
    std::fs::create_dir_all(&invalid_hex_shard).expect("invalid hex shard");
    let invalid_hex_entry = invalid_hex_shard.join(format!("ab{}g.json", "a".repeat(61)));
    std::fs::write(&invalid_hex_entry, b"x").expect("foreign file");

    cache.evict_if_needed().expect("eviction");

    assert!(
        wrong_shard_entry.exists(),
        "digest belongs to another shard"
    );
    assert!(invalid_hex_entry.exists(), "digest is not lower-case hex");
}
