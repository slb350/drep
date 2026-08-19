//! Unit tests for [`crate::cli::doctor`].
//!
//! Acceptance tests (criteria A1-A10) live in `tests/`. This file pins
//! `unset_env_vars`, whose grammar has to agree with `config`'s substituter -
//! a disagreement there makes `doctor` report a broken config as fine, which
//! is the one thing this command must never do.

use crate::cli::doctor::unset_env_vars;

/// Parse `body` as TOML for the scanner tests.
fn tree(body: &str) -> toml::Value {
    toml::from_str(body).expect("fixture parses")
}

/// References are deduped, and a name that IS set is filtered out.
///
/// The set half is what the previous version of this test could not check -
/// it deferred to "acceptance test A7", which does not exist. Deleting the
/// `.filter(...)` therefore passed the whole suite, and every `${VAR}` in a
/// config would have been reported as unset. `CARGO_PKG_NAME` is set by cargo
/// for every test run, so the set case needs no `env::set_var` - which is
/// `unsafe` in edition 2024 and races the tests that spawn git.
#[test]
fn unset_env_vars_dedupes_and_filters_set_names() {
    let unset = unset_env_vars(&tree(
        "api_key = \"${DREP_TEST_DOCTOR_DEDUPE_UNSET_NEVER_SET_XYZ}\"\n\
         other_key = \"${DREP_TEST_DOCTOR_DEDUPE_UNSET_NEVER_SET_XYZ}\"\n",
    ));
    assert_eq!(
        unset,
        vec!["DREP_TEST_DOCTOR_DEDUPE_UNSET_NEVER_SET_XYZ".to_owned()],
        "two references to one name are one warning"
    );

    assert!(
        std::env::var("CARGO_PKG_NAME").is_ok(),
        "cargo sets this for every test run; the assertion below relies on it"
    );
    let unset = unset_env_vars(&tree("model = \"${CARGO_PKG_NAME}\"\n"));
    assert!(
        unset.is_empty(),
        "a variable that IS set must not be reported, got {unset:?}"
    );
}

/// The reference grammar is `config`'s, not a stricter one of doctor's own.
///
/// This test used to compile its own copy of doctor's regex and assert against
/// that, so it tested the `regex` crate and stayed green whatever the
/// implementation did. It is the reason the grammar drifted unnoticed:
/// `config::expand_string` substitutes *any* name between `${` and `}`, so a
/// lowercase one is a real reference that doctor silently ignored - while
/// suppressing `config::load`'s complaint about it.
#[test]
fn the_reference_grammar_matches_the_substituters_not_a_stricter_one() {
    let unset = unset_env_vars(&tree(
        "a = \"${lower_case_never_set_xyz}\"\nb = \"${Mixed_Case_Never_Set_Xyz}\"\n",
    ));
    assert_eq!(
        unset,
        vec![
            "lower_case_never_set_xyz".to_owned(),
            "Mixed_Case_Never_Set_Xyz".to_owned()
        ],
        "config::expand_string would fail on both, so doctor must warn about both"
    );

    // The closing brace ends the reference; trailing text is not part of it.
    let unset = unset_env_vars(&tree("a = \"${DREP_UNSET_XYZ}_TAIL\"\n"));
    assert_eq!(unset, vec!["DREP_UNSET_XYZ".to_owned()]);
}

/// A `${VAR}` inside a comment is documentation, not a reference.
///
/// Scanning the parsed tree rather than the file text is what makes this true;
/// a raw-text scan warned about variables the config never actually used.
#[test]
fn a_reference_inside_a_comment_is_not_reported() {
    let unset = unset_env_vars(&tree(
        "# example: api_key = \"${DREP_COMMENT_ONLY_NEVER_SET_XYZ}\"\nmodel = \"m\"\n",
    ));
    assert!(
        unset.is_empty(),
        "a commented-out example is not a reference, got {unset:?}"
    );
}
