//! Cache key composition.
//!
//! Criteria 1-7. Every test pins a property of the key derivation - the key
//! must be deterministic, sensitive to each input in turn, immune to
//! boundary confusion between fields, and independent of the cache
//! instance.

use crate::llm::cache::Cache;

fn cache_in(temp: &tempfile::TempDir) -> Cache {
    Cache::new(temp.path().to_path_buf(), 30, 1024 * 1024)
}

/// Criterion 1: identical inputs produce identical keys.
#[test]
fn identical_inputs_produce_identical_keys() {
    let temp = tempfile::tempdir().expect("tempdir");
    let cache = cache_in(&temp);

    let k1 = cache.key("sys", "content", "model", 0.2);
    let k2 = cache.key("sys", "content", "model", 0.2);

    assert_eq!(k1, k2, "identical inputs must hash identically");
}

/// Criterion 2: a different `content` produces a different key.
#[test]
fn different_content_produces_different_key() {
    let temp = tempfile::tempdir().expect("tempdir");
    let cache = cache_in(&temp);

    let k1 = cache.key("sys", "alpha", "model", 0.2);
    let k2 = cache.key("sys", "beta", "model", 0.2);

    assert_ne!(k1, k2, "different content must hash differently");
}

/// Criterion 3: a different `system_prompt` produces a different key.
#[test]
fn different_system_prompt_produces_different_key() {
    let temp = tempfile::tempdir().expect("tempdir");
    let cache = cache_in(&temp);

    let k1 = cache.key("prompt A", "content", "model", 0.2);
    let k2 = cache.key("prompt B", "content", "model", 0.2);

    assert_ne!(k1, k2, "different system_prompt must hash differently");
}

/// Criterion 4: a different `model` produces a different key.
#[test]
fn different_model_produces_different_key() {
    let temp = tempfile::tempdir().expect("tempdir");
    let cache = cache_in(&temp);

    let k1 = cache.key("sys", "content", "gpt-4", 0.2);
    let k2 = cache.key("sys", "content", "llama-3", 0.2);

    assert_ne!(k1, k2, "different model must hash differently");
}

/// Criterion 5: a different `temperature` produces a different key.
#[test]
fn different_temperature_produces_different_key() {
    let temp = tempfile::tempdir().expect("tempdir");
    let cache = cache_in(&temp);

    let k1 = cache.key("sys", "content", "model", 0.0);
    let k2 = cache.key("sys", "content", "model", 0.5);

    assert_ne!(k1, k2, "different temperature must hash differently");
}

/// Criterion 6: field boundaries cannot be confused.
///
/// `key("ab", "c", ...)` and `key("a", "bc", ...)` must produce different
/// keys. A naive concatenation that used a separator byte contained in
/// either field could collide here; length-prefixing is the safety net.
#[test]
fn field_boundaries_cannot_be_confused() {
    let temp = tempfile::tempdir().expect("tempdir");
    let cache = cache_in(&temp);

    let boundary_left = cache.key("ab", "c", "model", 0.2);
    let boundary_right = cache.key("a", "bc", "model", 0.2);

    assert_ne!(
        boundary_left, boundary_right,
        "key(\"ab\", \"c\") must not collide with key(\"a\", \"bc\")"
    );
}

/// Criterion 7: the key is stable across `Cache` instances with different
/// roots - it depends only on the four inputs.
#[test]
fn key_is_stable_across_cache_instances() {
    let temp_a = tempfile::tempdir().expect("tempdir");
    let temp_b = tempfile::tempdir().expect("tempdir");
    let cache_a = Cache::new(temp_a.path().to_path_buf(), 30, 1024);
    let cache_b = Cache::new(temp_b.path().to_path_buf(), 30, 1024 * 1024);

    let k_a = cache_a.key("sys", "content", "model", 0.2);
    let k_b = cache_b.key("sys", "content", "model", 0.2);

    assert_eq!(
        k_a, k_b,
        "the key must depend only on the four inputs, not on root/ttl/max_bytes"
    );
}
