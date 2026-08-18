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
        config.providers().first().and_then(|p| p.model.as_deref()),
        Some("first"),
        "the chain leads with the head of the list, not an arbitrary entry"
    );
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
        ConfigError::Temperature { index, temperature } => {
            assert_eq!(index, 1, "the second provider is the offender");
            assert!((temperature - 2.5).abs() < f32::EPSILON);
            // Rendered one-based and labelled, so it cannot be confused with
            // the chain's own one-based numbering of the enabled entries.
            let rendered = ConfigError::Temperature { index, temperature }.to_string();
            assert!(rendered.contains("#2 in file order"), "got {rendered:?}");
        }
        other => panic!("expected Temperature, got {other:?}"),
    }
}

/// `providers()` is the failover chain: enabled entries, in file order.
///
/// The discriminating case is a disabled entry in the *middle*. A filter that
/// stopped at the first disabled entry, or one that only skipped a disabled
/// head, both pass a two-entry fixture.
#[test]
fn providers_skips_disabled_entries_and_keeps_file_order() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = write_config(
        &temp,
        r#"
[[llm]]
model = "first"

[[llm]]
enabled = false
model = "parked"

[[llm]]
model = "third"
"#,
    );

    let config = load(&path).expect("load");
    let models: Vec<&str> = config
        .providers()
        .iter()
        .map(|p| p.model.as_deref().expect("model"))
        .collect();
    assert_eq!(
        models,
        vec!["first", "third"],
        "a disabled entry is skipped, and the survivors keep their order"
    );
}

/// A disabled *head* means the next enabled entry leads the chain.
///
/// This is the case the phase existed to fix: parking the local model was
/// supposed to fall through to the cloud entry below, and instead produced
/// `NotConfigured` because the head was consulted regardless of `enabled`.
#[test]
fn a_disabled_head_falls_through_to_the_next_enabled_entry() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = write_config(
        &temp,
        r#"
[[llm]]
enabled = false
model = "local"

[[llm]]
model = "cloud"
"#,
    );

    let config = load(&path).expect("load");
    assert_eq!(
        config.providers().first().and_then(|p| p.model.as_deref()),
        Some("cloud"),
        "the chain leads with the first ENABLED entry, not the first entry"
    );
}

/// Every entry disabled is rejected at load, not at the LLM boundary.
///
/// Same rule as an empty list: a config that can never produce a passing run
/// is reported by the code that read the file, naming the file.
#[test]
fn a_config_whose_every_provider_is_disabled_is_rejected() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = write_config(
        &temp,
        r#"
[[llm]]
enabled = false
model = "a"

[[llm]]
enabled = false
model = "b"
"#,
    );
    let err = load(&path).expect_err("an all-disabled config must fail");
    match err {
        ConfigError::NoEnabledProviders(reported) => assert_eq!(reported, path),
        other => panic!("expected NoEnabledProviders, got {other:?}"),
    }
}

/// An entry that does not mention `enabled` is in the chain.
///
/// `enabled` is an opt-*out*. Defaulting it to false made declaring a
/// provider do nothing until you also enabled it - so a user who hand-wrote a
/// second `[[llm]]` block by copying the first, minus the `enabled` line, got
/// a silently inert fallback and no failover.
#[test]
fn an_entry_without_an_enabled_key_participates() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = write_config(&temp, "[[llm]]\nmodel = \"x\"\n");
    let config = load(&path).expect("load");
    assert!(config.llm[0].enabled, "`enabled` defaults to true");
    assert_eq!(config.providers().len(), 1);
}

/// `providers()` on a directly-constructed empty `Config` is empty rather
/// than panicking. `load` cannot produce one, but `Config` is constructible,
/// and a panic inside a commit gate is worse than an error.
#[test]
fn providers_of_an_empty_config_is_empty() {
    assert!(Config::default().providers().is_empty());
}

/// A parked provider's unset `${VAR}` does not refuse to load the file.
///
/// `enabled = false` means the entry is inert. Refusing to load because a
/// provider the user just switched off names a variable they have not exported
/// contradicts that in the one place they would notice — switching it off is
/// exactly what they did to stop it mattering.
#[test]
fn a_disabled_provider_with_an_unset_env_var_still_loads() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = write_config(
        &temp,
        r#"
[[llm]]
model = "local"
endpoint = "http://localhost:1234/v1"

[[llm]]
enabled = false
model = "cloud"
endpoint = "https://api.example/v1"
api_key = "${DREP_TEST_VAR_THAT_IS_NOT_SET}"
"#,
    );

    let config = load(&path).expect("a parked provider's env var is not required");
    assert_eq!(config.providers().len(), 1);
    assert_eq!(config.providers()[0].model.as_deref(), Some("local"));
}

/// An *enabled* provider's unset `${VAR}` is still an error.
///
/// The discriminating half: a rule that skipped expansion everywhere would pass
/// the test above and silently hand a literal `${VAR}` to the endpoint as a
/// credential.
#[test]
fn an_enabled_provider_with_an_unset_env_var_is_still_rejected() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = write_config(
        &temp,
        r#"
[[llm]]
model = "cloud"
endpoint = "https://api.example/v1"
api_key = "${DREP_TEST_VAR_THAT_IS_NOT_SET}"
"#,
    );
    let err = load(&path).expect_err("an enabled provider needs its variable");
    match err {
        ConfigError::EnvVarUnset(name, _) => {
            assert_eq!(name, "DREP_TEST_VAR_THAT_IS_NOT_SET");
        }
        other => panic!("expected EnvVarUnset, got {other:?}"),
    }
}

/// A disabled provider's out-of-range temperature does not block the load.
#[test]
fn a_disabled_provider_is_not_validated() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = write_config(
        &temp,
        r#"
[[llm]]
model = "live"

[[llm]]
enabled = false
model = "parked"
temperature = 9.0
max_concurrent = 0
"#,
    );
    load(&path).expect("a parked provider's fields are not validated");
}

/// `max_concurrent = 0` is rejected rather than left to hang.
///
/// A semaphore with no permits never hands one out, so every request waits for
/// a slot forever - a commit gate that reports nothing and never returns. The
/// index names the offending block.
#[test]
fn zero_max_concurrent_is_rejected() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = write_config(
        &temp,
        "[[llm]]\nmodel = \"a\"\n\n[[llm]]\nmodel = \"b\"\nmax_concurrent = 0\n",
    );
    let err = load(&path).expect_err("zero concurrency must not load");
    match err {
        ConfigError::ZeroConcurrency { index } => {
            assert_eq!(index, 1, "the second provider is the offender");
            let rendered = ConfigError::ZeroConcurrency { index }.to_string();
            assert!(rendered.contains("#2 in file order"), "got {rendered:?}");
        }
        other => panic!("expected ZeroConcurrency, got {other:?}"),
    }
}
