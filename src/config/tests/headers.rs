//! `[llm.headers]`: the effective set, and what must never reach a log.
//!
//! Redaction is argued once, on the `LlmConfig::headers` field. What is pinned
//! here is that each printer obeys it.

use std::collections::BTreeMap;
use std::env;

use super::support::write_config;
use crate::config::{ConfigError, DEFAULT_USER_AGENT, LlmConfig, effective_headers, load};

/// The ordinary case: a table of extra headers on one provider.
#[test]
fn a_headers_table_is_parsed_into_the_entry() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = write_config(
        &temp,
        r#"
[[llm]]
model = "m"
endpoint = "http://e/v1"

[llm.headers]
"User-Agent" = "acme-gate/3.1"
"X-Tenant-Token" = "t-123"
"#,
    );

    let config = load(&path).expect("a headers table is valid");

    assert_eq!(config.llm[0].headers.len(), 2);
    assert_eq!(
        config.llm[0].headers.get("User-Agent").map(String::as_str),
        Some("acme-gate/3.1")
    );
}

/// A header value is a place a credential goes, so it takes `${VAR}` like every
/// other value in the file. Without it the only way to send a tenant token is to
/// write it into `drep.toml` in the clear.
#[test]
fn a_header_value_expands_an_environment_reference() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = write_config(
        &temp,
        r#"
[[llm]]
model = "m"
endpoint = "http://e/v1"

[llm.headers]
"X-Tenant-Token" = "${DREP_TEST_TENANT_TOKEN_UNSET}"
"#,
    );

    let err = load(&path).expect_err("an unset reference in a header value is reported");

    assert!(
        matches!(&err, ConfigError::EnvVarUnset(var, _) if var == "DREP_TEST_TENANT_TOKEN_UNSET"),
        "the expansion pass reaches header values: {err:?}"
    );
}

/// A header drep cannot send fails at load, once, naming the entry.
///
/// Left to the request it failed per file: `LlmError::NotConfigured` neither
/// fails over nor sticks, so a two-hundred-file diff reported the same typo two
/// hundred times, each one rendered as a transport failure - which reads as the
/// endpoint being down rather than as a config the user can fix.
#[test]
fn an_unsendable_header_name_is_rejected_at_load() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = write_config(
        &temp,
        "[[llm]]\nmodel = \"m\"\nendpoint = \"http://e/v1\"\nheaders = { \"Bad Name\" = \"v\" }\n",
    );

    let err = load(&path).expect_err("a header name with a space cannot be sent");

    assert!(
        matches!(&err, ConfigError::UnusableHeaderName { name, .. } if name == "Bad Name"),
        "got {err:?}"
    );
    assert!(
        err.to_string().contains("#1 in file order"),
        "and names the entry like every sibling rule: {err}"
    );
}

/// The value side is the data-dependent one, and the one a name check misses.
///
/// A `${TENANT_TOKEN}` whose expansion picked up a stray newline is the case
/// that passes load, passes `doctor`, and then fails every request in the run.
#[test]
fn an_unsendable_header_value_is_rejected_without_being_quoted_back() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = write_config(
        &temp,
        "[[llm]]\nmodel = \"m\"\nendpoint = \"http://e/v1\"\n\
         headers = { \"X-Tenant-Token\" = \"secret\\nvalue\" }\n",
    );

    let err = load(&path).expect_err("a control character cannot be sent in a header");
    let message = err.to_string();

    assert!(
        matches!(&err, ConfigError::UnusableHeaderValue { name, .. } if name == "X-Tenant-Token"),
        "got {err:?}"
    );
    assert!(
        !message.contains("secret"),
        "the value is the half that carries the token: {message}"
    );
}

/// The rule runs after `${VAR}` expansion, and that ordering is the whole of it.
///
/// A control character written literally into `drep.toml` is visible in the
/// file. One arriving through a `${VAR}` - a helper appending a newline to the
/// token it mints - is not, and that is the case that happens. Re-implemented as
/// a raw-tree pass before expansion, the shape `load` already uses twice for
/// `site_only_field` and `backend::explicit_fields`, this rule would still pass
/// every literal test above and let that token through to fail identically on
/// every file of the run.
#[test]
fn a_header_value_is_checked_after_its_environment_reference_expands() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = write_config(
        &temp,
        "[[llm]]\nmodel = \"m\"\nendpoint = \"http://e/v1\"\n\
         headers = { \"X-Tenant-Token\" = \"${DREP_TEST_UNSENDABLE_TOKEN}\" }\n",
    );

    // A name unique to this test, so a parallel run or a variable leaked from
    // elsewhere cannot poison the assertion, and restored on both branches so the
    // surrounding tests stay deterministic.
    let var = "DREP_TEST_UNSENDABLE_TOKEN";
    let previous = env::var(var).ok();
    // SAFETY: the process is single-threaded at this point and the name is
    // unique to this test.
    unsafe { env::set_var(var, "secret\nvalue") };
    let result = load(&path);
    // SAFETY: see above.
    match previous {
        Some(prev) => unsafe { env::set_var(var, prev) },
        None => unsafe { env::remove_var(var) },
    }

    let err = result.expect_err("the expanded token cannot be sent in a header");
    let message = err.to_string();

    assert!(
        matches!(&err, ConfigError::UnusableHeaderValue { name, .. } if name == "X-Tenant-Token"),
        "the check reads the expanded value, not the `${{VAR}}` that stood in for it: {err:?}"
    );
    assert!(
        !message.contains("secret"),
        "and still never quotes the token back: {message}"
    );
}

/// Two spellings of one header name are refused rather than silently resolved.
///
/// They are two keys to a `BTreeMap` and one header to HTTP, so the map keeps
/// both, `doctor` and both `Debug` impls list both, and the request sends
/// whichever the SDK's case-insensitive `HeaderMap` happens to insert last -
/// which is byte order, not anything the user wrote. With `Authorization` and
/// `authorization` both configured that decides which credential goes out.
#[test]
fn two_spellings_of_one_header_name_are_rejected_at_load() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = write_config(
        &temp,
        "[[llm]]\nmodel = \"m\"\nendpoint = \"http://e/v1\"\n\
         headers = { \"X-Tenant-Token\" = \"prod-token\", \"x-tenant-token\" = \"staging-token\" }\n",
    );

    let err = load(&path).expect_err("one header name written twice is ambiguous");
    let message = err.to_string();

    assert!(
        matches!(
            &err,
            ConfigError::DuplicateHeaderName { first, second, .. }
                if first == "X-Tenant-Token" && second == "x-tenant-token"
        ),
        "got {err:?}"
    );
    assert!(
        message.contains("X-Tenant-Token") && message.contains("x-tenant-token"),
        "both spellings, so the user can see which one to remove: {message}"
    );
    assert!(
        !message.contains("prod-token") && !message.contains("staging-token"),
        "and neither value, because either could be the credential: {message}"
    );
}

/// The silent-drop fix, and the reason this branch exists.
///
/// Before `deny_unknown_fields`, a `[llm.headers]` table written against a drep
/// that could not send one was accepted and dropped without a word: the run
/// completed, the findings looked normal, and the headers were never sent.
#[test]
fn an_unknown_provider_field_is_rejected_rather_than_dropped() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = write_config(
        &temp,
        "[[llm]]\nmodel = \"m\"\nendpoint = \"http://e/v1\"\nheaderz = { \"X-Typo\" = \"v\" }\n",
    );

    let err = load(&path).expect_err("a misspelled field is not silently dropped");
    let message = err.to_string();

    assert!(
        message.contains("headerz"),
        "the message names the field that was not understood: {message}"
    );
    assert!(
        message.contains("headers"),
        "and lists what was expected, which is the fix: {message}"
    );
}

/// The same hazard one level up, which the first pass at this left untouched.
#[test]
fn an_unknown_top_level_field_is_rejected_rather_than_dropped() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = write_config(
        &temp,
        "max_reveiw_rounds = 99\n\n[[llm]]\nmodel = \"m\"\nendpoint = \"http://e/v1\"\n",
    );

    let err = load(&path).expect_err("a misspelled top-level key ran at the default silently");

    assert!(err.to_string().contains("max_reveiw_rounds"), "got {err}");
}

/// A policy key still answers as a policy key rather than as a generic typo.
///
/// `site_only_field` runs against the raw tree before deserialization, and
/// `deny_unknown_fields` could have taken the message over.
#[test]
fn a_site_only_field_keeps_its_own_message_under_deny_unknown_fields() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = write_config(
        &temp,
        "refuse_markers = [\".drep-no-llm\"]\n\n[[llm]]\nmodel = \"m\"\nendpoint = \"http://e/v1\"\n",
    );

    let err = load(&path).expect_err("a repository cannot declare site policy");

    assert!(
        matches!(&err, ConfigError::SiteOnlyField { .. }),
        "the specific rule wins over the generic one: {err:?}"
    );
}

/// A header value can be a credential, so `{:?}` prints names and nothing else.
#[test]
fn debug_prints_header_names_and_never_their_values() {
    let config = LlmConfig {
        headers: BTreeMap::from([("X-Tenant-Token".to_owned(), "super-secret-value".to_owned())]),
        ..LlmConfig::default()
    };

    let rendered = format!("{config:?}");

    assert!(
        rendered.contains("X-Tenant-Token"),
        "the name is the useful non-secret half: {rendered}"
    );
    assert!(
        !rendered.contains("super-secret-value"),
        "a header value must never reach a log: {rendered}"
    );
}

/// Unconfigured, drep still names itself.
#[test]
fn the_effective_set_carries_a_default_user_agent() {
    let effective = effective_headers(&BTreeMap::new());

    assert_eq!(
        effective.get("User-Agent").map(String::as_str),
        Some(DEFAULT_USER_AGENT)
    );
    assert!(DEFAULT_USER_AGENT.starts_with("drep/"));
}

/// A configured user agent replaces the default rather than joining it, whatever
/// case it is written in, because HTTP header names are case-insensitive and the
/// SDK's `HeaderMap` would collapse them at send time anyway.
#[test]
fn a_configured_user_agent_replaces_the_default_whatever_its_case() {
    let configured = BTreeMap::from([("user-agent".to_owned(), "acme-gate/3.1".to_owned())]);

    let effective = effective_headers(&configured);

    assert_eq!(effective.len(), 1, "one user agent, not two: {effective:?}");
    assert_eq!(
        effective.get("user-agent").map(String::as_str),
        Some("acme-gate/3.1")
    );
}
