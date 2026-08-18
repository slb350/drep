//! The `[[llm]]` array-of-tables shape.
//!
//! Providers are an ordered *list* from the day `drep init` first wrote one,
//! even though only the head is consulted today — multi-provider failover
//! fills the tail later, and a format change underneath a file drep itself
//! wrote is the thing this shape exists to avoid.

use super::support::write_config;
use crate::config::*;

#[test]
fn a_config_with_no_llm_provider_is_rejected() {
    let temp = tempfile::tempdir().expect("tempdir");
    for body in ["", "# only a comment\n"] {
        let path = write_config(&temp, body);
        let err = load(&path).expect_err("a provider-less config must fail");
        match err {
            ConfigError::NoProviders(reported) => assert_eq!(reported, path),
            other => panic!("expected NoProviders, got {other:?}"),
        }
    }
}

/// The old single-table `[llm]` shape no longer parses.
///
/// This is the discriminating half of the array change: without it, a
/// suite full of `[[llm]]` fixtures would pass just as well against a
/// `Config` that still held one `LlmConfig`, because TOML would happily
/// feed the first table to it.

#[test]
fn the_single_table_llm_shape_is_a_parse_error() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = write_config(&temp, "[llm]\nmodel = \"x\"\n");
    let err = load(&path).expect_err("`[llm]` is not `[[llm]]`");
    assert!(
        matches!(err, ConfigError::Parse(_, _)),
        "expected a Parse error for the single-table shape, got {err:?}"
    );
}

/// Several providers are kept, in file order, and `primary` is the first.
///
/// Ordering is the contract failover will rely on: the list is a
/// preference order, not a set.

#[test]
fn providers_are_kept_in_file_order_and_primary_is_the_first() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = write_config(
        &temp,
        r#"
[[llm]]
model = "first"
endpoint = "http://a/v1"

[[llm]]
model = "second"
endpoint = "http://b/v1"

[[llm]]
model = "third"
endpoint = "http://c/v1"
"#,
    );

    let config = load(&path).expect("load");
    let models: Vec<&str> = config
        .llm
        .iter()
        .map(|p| p.model.as_deref().expect("model"))
        .collect();
    assert_eq!(models, vec!["first", "second", "third"]);
    assert_eq!(
        config.primary().and_then(|p| p.model.as_deref()),
        Some("first"),
        "primary is the head of the list, not an arbitrary entry"
    );
}

/// `primary()` on a directly-constructed empty `Config` returns `None`
/// rather than panicking. `load` cannot produce one, but `Config` is
/// constructible, and a panic inside a commit gate is worse than an error.

#[test]
fn primary_of_an_empty_config_is_none() {
    assert!(Config::default().primary().is_none());
}

/// The temperature error names *which* provider is out of range.
///
/// With one provider the index is trivially 0 and proves nothing; the
/// second entry being the bad one is what pins that the index is the
/// offender's rather than a constant.

#[test]
fn temperature_error_carries_the_offending_provider_index() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = write_config(
        &temp,
        r#"
[[llm]]
model = "fine"
temperature = 0.2

[[llm]]
model = "too-hot"
temperature = 2.5
"#,
    );
    let err = load(&path).expect_err("out-of-range temperature must fail");
    match err {
        ConfigError::Temperature(index, value) => {
            assert_eq!(index, 1, "the second provider is the offender");
            assert!((value - 2.5).abs() < f32::EPSILON);
        }
        other => panic!("expected Temperature, got {other:?}"),
    }
}
