//! `${VAR}` expansion.
//!
//! The rule is `${NAME}` exactly: an unset variable is an error rather than an
//! empty string, because a silently-empty credential produces a confusing 401
//! instead of a clear "that variable is not set". A bare `$` survives verbatim
//! so paths and shell syntax are not mangled.

use super::support::write_config;
use crate::config::env::expand_env_in;
use crate::config::*;
use std::env;
use std::path::Path;

/// `expand_env_in` walks arrays as well as tables.
///
/// `api_key_command` is the field that reaches this arm through `load()`; the
/// walk predates it, which is why the arm survived mutation testing for a
/// while. It exists so an array-valued field inherits `${VAR}` expansion
/// without anyone remembering to opt in, and this pins the arm directly rather
/// than only through the one field that happens to use it.
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
fn an_empty_environment_reference_is_a_parse_error() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = write_config(&temp, "[[llm]]\napi_key = \"${}\"\n");

    let err = load(&path).expect_err("an empty variable name is not a reference");
    assert!(
        matches!(err, ConfigError::Parse(_, ref message) if message.contains("empty environment variable")),
        "got {err:?}"
    );
}

#[cfg(unix)]
#[test]
fn a_non_utf8_environment_value_is_not_reported_as_unset() {
    use std::os::unix::ffi::OsStringExt;

    let temp = tempfile::tempdir().expect("tempdir");
    let var = "DREP_TEST_NON_UTF8_ENV_VALUE_XYZ";
    let path = write_config(&temp, &format!("[[llm]]\napi_key = \"${{{var}}}\"\n"));
    let previous = env::var_os(var);
    // SAFETY: this module is the explicitly isolated environment-expansion
    // suite, and the unique variable is restored before the assertion.
    unsafe { env::set_var(var, std::ffi::OsString::from_vec(vec![0xff])) };
    let result = load(&path);
    match previous {
        Some(value) => unsafe { env::set_var(var, value) },
        None => unsafe { env::remove_var(var) },
    }

    let err = result.expect_err("the value cannot be represented in TOML text");
    assert!(
        matches!(err, ConfigError::EnvVarNotUnicode(ref name, _) if name == var),
        "got {err:?}"
    );
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

#[test]
fn a_var_reference_inside_an_api_key_command_expands_and_is_required() {
    // An argv element is a string like any other, so the walk already reaches
    // it. What has to hold is that `doctor` and `load` agree about it: a
    // reference only `load` knew about is the drift `required_env_var_refs`
    // exists to prevent.
    let temp = tempfile::tempdir().expect("tempdir");
    let var = "DREP_TEST_KEY_COMMAND_REF_XYZ";
    let path = write_config(
        &temp,
        &format!(
            "[[llm]]\nendpoint = \"https://gateway.example/v1\"\nmodel = \"m\"\n\
             api_key_command = [\"read-secret\", \"${{{var}}}\"]\n"
        ),
    );

    let tree: Value =
        toml::from_str(&std::fs::read_to_string(&path).expect("read")).expect("parse");
    assert_eq!(
        required_env_var_refs(&tree),
        vec![var.to_owned()],
        "doctor must see the reference the substituter will act on"
    );

    let previous = env::var_os(var);
    // SAFETY: this module is the isolated environment-expansion suite and the
    // variable name is unique to this test; it is restored before asserting.
    unsafe { env::set_var(var, "vault://secret") };
    let result = load(&path);
    match previous {
        Some(value) => unsafe { env::set_var(var, value) },
        None => unsafe { env::remove_var(var) },
    }

    let config = result.expect("load");
    assert_eq!(
        config.llm[0].api_key_command.as_deref(),
        Some(["read-secret".to_owned(), "vault://secret".to_owned()].as_slice())
    );
}
