//! Eviction behaviour.
//!
//! Criteria 18-20. Three pins: `evict_if_needed` reduces the tree under
//! the limit and reports bytes freed; the oldest entry is removed first;
//! a tree already under the limit is a no-op returning 0.

use std::time::SystemTime;

use serde_json::json;

use crate::llm::cache::Cache;

/// A side-effect-free cache constructor leaves eviction with no tree to walk.
///
/// Missing is the ordinary empty-cache state, not a walk failure, and eviction
/// must not create the root merely to report that it freed nothing.
#[test]
fn eviction_on_an_absent_cache_is_a_side_effect_free_no_op() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("absent-cache");
    let cache = Cache::new(root.clone(), 30, 60);

    assert_eq!(cache.evict_if_needed().expect("missing means empty"), 0);
    assert!(
        !root.exists(),
        "eviction must not create an empty cache root"
    );
}

/// Backdate `path`'s mtime so we can write two entries with distinct ages
/// for the "oldest first" test.
fn set_mtime(path: &std::path::Path, mtime: SystemTime) {
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .expect("open for mtime set");
    file.set_modified(mtime).expect("set_modified");
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

/// Eviction never touches a directory that is not one of ours.
///
/// `evict_if_needed` is the only destructive path in this module, and it walks
/// whatever sits under the cache root. Checking `is_dir()` alone - while the
/// comment claimed the walk was restricted to two-hex-char shards - meant a
/// directory a user had placed under the root had its files deleted to make
/// room. The size accounting must ignore it too, or a foreign directory's bytes
/// push real entries out.
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

    // Two things that are not shards: a stray file in the root, and a
    // directory whose name is not two hex characters.
    let stray_file = root.join("notes.txt");
    std::fs::write(&stray_file, b"do not delete me").expect("stray file");
    let foreign_dir = root.join("zz");
    std::fs::create_dir_all(&foreign_dir).expect("foreign dir");
    let foreign_file = foreign_dir.join("important.bin");
    std::fs::write(&foreign_file, vec![0u8; 4096]).expect("foreign file");

    cache.evict_if_needed().expect("eviction runs");

    assert!(
        foreign_file.exists(),
        "a directory that is not a shard must not be walked, let alone emptied"
    );
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
