//! Unit tests for `drep auth`.
//!
//! Every case runs against a store inside a `TempDir`. Using the real one would
//! read and rewrite the developer's own keys, and would make the assertions
//! depend on whose machine the suite ran on.

use super::*;
use crate::auth::AuthStore;

/// A store path in a fresh temp dir, plus the dir to keep it alive.
fn temp_store() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("auth.toml");
    (dir, path)
}

/// Run `command` against `path` and return what it printed.
fn output(command: AuthCommand, path: &Path) -> String {
    let mut out = Vec::new();
    run_at(&mut out, &AuthArgs { command }, path).expect("the command succeeds");
    String::from_utf8(out).expect("utf8")
}

#[test]
fn list_says_so_when_nothing_is_stored() {
    let (_dir, path) = temp_store();

    let rendered = output(AuthCommand::List, &path);

    assert!(rendered.contains("No keys stored"), "got {rendered}");
    assert!(
        rendered.contains("drep auth login"),
        "and says what to do about it: {rendered}"
    );
}

#[test]
fn list_prints_the_endpoints_and_never_the_keys() {
    // The reason there is no `drep auth show`: this output is what a user pastes
    // into a bug report.
    let (_dir, path) = temp_store();
    let mut store = AuthStore::new();
    store
        .set("https://api.kimi.com/coding/v1", "sk-very-secret")
        .expect("set");
    store.save(&path).expect("save");

    let rendered = output(AuthCommand::List, &path);

    assert!(
        rendered.contains("https://api.kimi.com/coding/v1"),
        "got {rendered}"
    );
    assert!(
        !rendered.contains("sk-very-secret"),
        "the key was printed: {rendered}"
    );
}

#[test]
fn list_names_the_preset_serving_an_endpoint_it_recognises() {
    // A bare URL reads as something half-remembered; the preset name is what a
    // user recognises as the thing they set up.
    let (_dir, path) = temp_store();
    let mut store = AuthStore::new();
    store
        .set("https://api.kimi.com/coding/v1", "k")
        .expect("set");
    store.save(&path).expect("save");

    let rendered = output(AuthCommand::List, &path);

    let display = presets::preset("kimi").expect("preset").display_name;
    assert!(rendered.contains(display), "got {rendered}");
}

#[test]
fn list_prints_an_unrecognised_endpoint_without_inventing_a_name() {
    let (_dir, path) = temp_store();
    let mut store = AuthStore::new();
    store
        .set("https://something.internal/v1", "k")
        .expect("set");
    store.save(&path).expect("save");

    let rendered = output(AuthCommand::List, &path);

    assert!(
        rendered.contains("https://something.internal/v1"),
        "got {rendered}"
    );
    assert!(
        !rendered.contains('('),
        "no preset name to print: {rendered}"
    );
}

#[test]
fn logout_forgets_a_stored_key() {
    let (_dir, path) = temp_store();
    let mut store = AuthStore::new();
    store.set("https://e/v1", "k").expect("set");
    store.save(&path).expect("save");

    let rendered = output(
        AuthCommand::Logout(LogoutArgs {
            endpoint: "https://e/v1".to_string(),
        }),
        &path,
    );

    assert!(rendered.contains("Forgot"), "got {rendered}");
    assert!(
        AuthStore::load(&path)
            .expect("reload")
            .get("https://e/v1")
            .is_none(),
        "and the change reached the file"
    );
}

#[test]
fn logout_on_an_endpoint_with_no_key_says_so_rather_than_failing() {
    // Removing something that is not there is not an error, but reporting it as
    // a success would leave a user believing a key was cleared when the one they
    // meant is still stored under a different spelling.
    let (_dir, path) = temp_store();

    let rendered = output(
        AuthCommand::Logout(LogoutArgs {
            endpoint: "https://e/v1".to_string(),
        }),
        &path,
    );

    assert!(rendered.contains("No key was stored"), "got {rendered}");
}

#[test]
fn logout_matches_the_endpoint_the_way_the_store_stored_it() {
    let (_dir, path) = temp_store();
    let mut store = AuthStore::new();
    store.set("https://e/v1", "k").expect("set");
    store.save(&path).expect("save");

    output(
        AuthCommand::Logout(LogoutArgs {
            endpoint: "https://E/v1/".to_string(),
        }),
        &path,
    );

    assert!(
        AuthStore::load(&path).expect("reload").is_empty(),
        "the same endpoint, spelled differently"
    );
}

#[test]
fn a_provider_name_resolves_to_that_presets_endpoint() {
    // The two ways of naming an endpoint must agree, or `drep init --provider
    // kimi` and `drep auth login --provider kimi` store keys in different slots.
    let resolved = resolve_endpoint(&LoginArgs {
        endpoint: None,
        provider: Some("kimi".to_string()),
    })
    .expect("resolves");

    assert_eq!(
        resolved,
        presets::preset("kimi")
            .expect("preset")
            .endpoint
            .expect("endpoint")
    );
}

#[test]
fn an_explicit_endpoint_is_used_as_given() {
    let resolved = resolve_endpoint(&LoginArgs {
        endpoint: Some("https://mine/v1".to_string()),
        provider: None,
    })
    .expect("resolves");

    assert_eq!(resolved, "https://mine/v1");
}

#[test]
fn naming_neither_is_an_error_that_says_what_to_pass() {
    let err = resolve_endpoint(&LoginArgs {
        endpoint: None,
        provider: None,
    })
    .expect_err("one of the two is required");

    let message = err.to_string();
    assert!(message.contains("--endpoint"), "got {message}");
    assert!(message.contains("--provider"), "got {message}");
}

#[test]
fn an_unknown_provider_is_named_in_the_error() {
    let err = resolve_endpoint(&LoginArgs {
        endpoint: None,
        provider: Some("nope".to_string()),
    })
    .expect_err("unknown preset");

    assert!(err.to_string().contains("nope"), "got {err}");
}

#[test]
fn a_preset_with_no_endpoint_cannot_supply_one() {
    // `custom` presumes no host, so there is nothing to key a credential on.
    let err = resolve_endpoint(&LoginArgs {
        endpoint: None,
        provider: Some("custom".to_string()),
    })
    .expect_err("custom has no endpoint");

    assert!(err.to_string().contains("--endpoint"), "got {err}");
}

#[test]
fn the_matching_preset_is_found_regardless_of_spelling() {
    let preset = matching_preset("https://API.KIMI.COM/coding/v1/").expect("matches");

    assert_eq!(preset.key, "kimi");
}

#[test]
fn an_endpoint_no_preset_serves_matches_nothing() {
    assert!(matching_preset("https://elsewhere/v1").is_none());
}
