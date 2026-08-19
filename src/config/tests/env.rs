//! `${VAR}` expansion.
//!
//! The rule is `${NAME}` exactly: an unset variable is an error rather than an
//! empty string, because a silently-empty credential produces a confusing 401
//! instead of a clear "that variable is not set". A bare `$` survives verbatim
//! so paths and shell syntax are not mangled.

use super::support::write_config;
use crate::config::*;
use std::env;
use std::path::Path;

/// `expand_env_in` walks arrays as well as tables.
///
/// No field in `LlmConfig` is an array today, so this cannot be reached
/// through `load()` — which is exactly why the array arm survived
/// mutation testing. It is still load-bearing: the walk exists so a future
/// array-valued field inherits `${VAR}` expansion without anyone
/// remembering to opt in, and a silently-skipped arm would break that
/// promise the moment such a field is added.
#[test]
fn env_expansion_descends_into_arrays() {
    // SAFETY: single-threaded test process; no other thread reads env here.
    unsafe { env::set_var("DREP_ARRAY_PROBE", "expanded") };

    let mut tree: Value = toml::from_str(
        r#"
values = ["${DREP_ARRAY_PROBE}", "literal"]
nested = { inner = ["${DREP_ARRAY_PROBE}"] }
"#,
    )
    .expect("fixture parses");

    expand_env_in(&mut tree, Path::new("probe.toml")).expect("expansion succeeds");

    let values = tree["values"].as_array().expect("array");
    assert_eq!(values[0].as_str(), Some("expanded"));
    assert_eq!(values[1].as_str(), Some("literal"));

    let inner = tree["nested"]["inner"].as_array().expect("nested array");
    assert_eq!(
        inner[0].as_str(),
        Some("expanded"),
        "arrays nested inside tables must expand too"
    );
}

#[test]
fn env_var_in_api_key_expands_from_environment() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = write_config(
        &temp,
        r#"
[[llm]]
api_key = "${DREP_TEST_API_KEY_VAR}"
"#,
    );

    // Use a name unique to this test so a parallel run or a leaked env
    // var from elsewhere cannot poison the assertion.
    let var = "DREP_TEST_API_KEY_VAR";
    // SAFETY: the test is single-threaded at this point and the var name
    // is unique to this test. We restore on either branch to keep the
    // surrounding tests deterministic.
    let previous = env::var(var).ok();
    // SAFETY: see above.
    unsafe {
        env::set_var(var, "expanded-secret-value");
    }
    let result = load(&path);
    if let Some(prev) = previous {
        unsafe {
            env::set_var(var, prev);
        }
    } else {
        unsafe {
            env::remove_var(var);
        }
    }

    let config = result.expect("load");
    assert_eq!(
        config.llm[0].api_key.as_deref(),
        Some("expanded-secret-value")
    );
}

#[test]
fn env_var_with_unset_variable_is_an_error_not_an_empty_string() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = write_config(
        &temp,
        r#"
[[llm]]
api_key = "${DREP_DEFINITELY_NOT_SET_VAR_XYZ_123}"
"#,
    );

    // Make sure no other test or shell leaked the variable.
    let previous = env::var("DREP_DEFINITELY_NOT_SET_VAR_XYZ_123").ok();
    unsafe {
        env::remove_var("DREP_DEFINITELY_NOT_SET_VAR_XYZ_123");
    }
    let result = load(&path);
    if let Some(prev) = previous {
        unsafe {
            env::set_var("DREP_DEFINITELY_NOT_SET_VAR_XYZ_123", prev);
        }
    }

    let err = result.expect_err("unset variable must fail");
    match err {
        ConfigError::EnvVarUnset(name, _) => {
            assert_eq!(name, "DREP_DEFINITELY_NOT_SET_VAR_XYZ_123");
        }
        other => panic!("expected EnvVarUnset, got {other:?}"),
    }
}

#[test]
fn literal_api_key_passes_through_unchanged() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = write_config(
        &temp,
        r#"
[[llm]]
api_key = "this-is-not-a-template"
"#,
    );

    let config = load(&path).expect("load");
    assert_eq!(
        config.llm[0].api_key.as_deref(),
        Some("this-is-not-a-template")
    );
}

#[test]
fn literal_dollar_not_followed_by_brace_is_preserved() {
    // The expansion rule is `${VAR}` exactly. A bare `$5` or `$HOME`
    // (without braces) must survive so filenames and shell syntax stay
    // intact.
    let temp = tempfile::tempdir().expect("tempdir");
    let path = write_config(
        &temp,
        r#"
[[llm]]
endpoint = "http://host/$1/path"
"#,
    );

    let config = load(&path).expect("load");
    assert_eq!(
        config.llm[0].endpoint.as_deref(),
        Some("http://host/$1/path")
    );
}
