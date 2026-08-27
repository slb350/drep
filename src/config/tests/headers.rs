//! `[llm.headers]`: the effective set, and what must never reach a log.
//!
//! Redaction is argued once, on the `LlmConfig::headers` field. What is pinned
//! here is that each printer obeys it.

use std::collections::BTreeMap;

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
