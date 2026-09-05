//! Cache key composition.
//!
//! Keys are deterministic, sensitive to every input, unambiguous at field
//! boundaries, and independent of cache storage settings. No test here uses disk.

use crate::llm::cache::{Cache, CacheKey};

const FIELDS: [&str; 5] = ["sys", "content", "http://endpoint/v1", "model", "openai"];

fn key(fields: [&str; 5], temperature: Option<f32>) -> CacheKey {
    let [system, content, backend, model, identity] = fields;
    Cache::new("unused-key-cache".into(), 30, 1024).key(
        system,
        content,
        backend,
        model,
        identity,
        temperature,
    )
}

#[test]
fn identical_inputs_produce_identical_keys() {
    for temperature in [None, Some(0.2)] {
        assert_eq!(key(FIELDS, temperature), key(FIELDS, temperature));
    }
}

#[test]
fn every_string_input_changes_the_key_independently() {
    let original = key(FIELDS, Some(0.2));
    for (index, name, replacement) in [
        (0, "system prompt", "another prompt"),
        (1, "content", "another body"),
        (2, "endpoint", "https://other.example/v1"),
        (3, "model", "another-model"),
        (4, "protocol", "anthropic"),
    ] {
        let mut changed = FIELDS;
        changed[index] = replacement;
        assert_ne!(original, key(changed, Some(0.2)), "changing {name}");
    }
}

#[test]
fn different_temperatures_including_unset_have_distinct_keys() {
    let values = [None, Some(0.0), Some(0.2), Some(0.5), Some(1.0)];
    for (index, &left) in values.iter().enumerate() {
        for &right in &values[index + 1..] {
            assert_ne!(
                key(FIELDS, left),
                key(FIELDS, right),
                "temperatures {left:?} and {right:?}"
            );
        }
    }
}

/// Criterion 6: field boundaries cannot be confused.
///
/// `key("ab", "c", ...)` and `key("a", "bc", ...)` must produce different
/// keys. A naive concatenation that used a separator byte contained in
/// either field could collide here; length-prefixing is the safety net.
#[test]
fn field_boundaries_cannot_be_confused() {
    let mut left = FIELDS;
    left[0] = "ab";
    left[1] = "c";
    let mut right = FIELDS;
    right[0] = "a";
    right[1] = "bc";

    assert_ne!(
        key(left, Some(0.2)),
        key(right, Some(0.2)),
        "key(\"ab\", \"c\") must not collide with key(\"a\", \"bc\")"
    );
}

/// Storage settings cannot affect request identity.
#[test]
fn key_is_stable_across_cache_instances() {
    let cache_a = Cache::new("unused-cache-a".into(), 1, 1024);
    let cache_b = Cache::new("unused-cache-b".into(), 30, 1024 * 1024);

    let k_a = cache_a.key(
        "sys",
        "content",
        "http://endpoint/v1",
        "model",
        "openai",
        Some(0.2),
    );
    let k_b = cache_b.key(
        "sys",
        "content",
        "http://endpoint/v1",
        "model",
        "openai",
        Some(0.2),
    );

    assert_eq!(k_a, k_b, "the key must not depend on root/ttl/max_bytes");
}
