//! Reading, writing, normalising and redacting the store itself.

use super::super::*;

/// A store path inside a fresh temp dir, plus the dir to keep it alive.
fn temp_store() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("drep").join("auth.toml");
    (dir, path)
}

#[test]
fn a_missing_file_loads_as_an_empty_store() {
    // Never having stored a key is the normal first-run state. Reporting it as a
    // read failure would put that branch at every call site.
    let (_dir, path) = temp_store();

    let store = AuthStore::load(&path).expect("a missing store is not an error");

    assert!(store.is_empty());
    assert_eq!(store.endpoints(), Vec::<&str>::new());
}

#[test]
fn a_stored_key_round_trips_through_the_file() {
    let (_dir, path) = temp_store();
    let mut store = AuthStore::new();
    store
        .set("https://api.kimi.com/coding/v1", "k-1")
        .expect("set");
    store.save(&path).expect("save");

    let reloaded = AuthStore::load(&path).expect("load");

    assert_eq!(reloaded.get("https://api.kimi.com/coding/v1"), Some("k-1"));
}

#[test]
fn a_corrupt_store_is_an_error_rather_than_an_empty_one() {
    // Treating unparseable content as "no keys stored" would send a user to
    // re-paste keys they already have, and silently.
    let (_dir, path) = temp_store();
    std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    std::fs::write(&path, "this is not toml {{{").expect("write");

    let err = AuthStore::load(&path).expect_err("a corrupt store is fatal");

    assert!(
        matches!(err, AuthError::Parse(..)),
        "expected a parse error, got {err:?}"
    );
}

#[test]
fn saving_creates_the_directory() {
    let (_dir, path) = temp_store();
    let mut store = AuthStore::new();
    store.set("https://e/v1", "k").expect("set");

    store.save(&path).expect("save creates its parent");

    assert!(path.exists());
}

#[cfg(unix)]
#[test]
fn the_saved_file_is_readable_only_by_its_owner() {
    use std::os::unix::fs::PermissionsExt;

    let (_dir, path) = temp_store();
    let mut store = AuthStore::new();
    store.set("https://e/v1", "k").expect("set");
    store.save(&path).expect("save");

    let mode = std::fs::metadata(&path)
        .expect("metadata")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600, "got {:o}", mode & 0o777);
}

#[cfg(unix)]
#[test]
fn the_saved_directory_is_enterable_only_by_its_owner() {
    use std::os::unix::fs::PermissionsExt;

    let (_dir, path) = temp_store();
    let mut store = AuthStore::new();
    store.set("https://e/v1", "k").expect("set");
    store.save(&path).expect("save");

    let parent = path.parent().expect("parent");
    let mode = std::fs::metadata(parent)
        .expect("metadata")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o700, "got {:o}", mode & 0o777);
}

#[cfg(unix)]
#[test]
fn saving_does_not_narrow_an_existing_directory() {
    // The runner service has a 0077 umask, so a freshly-created directory is
    // already 0700 even if `ensure_dir_private` fails to call `restrict`.
    // Widening an existing directory makes the ownership rule observable
    // without depending on the process umask.
    use std::os::unix::fs::PermissionsExt;

    let (_dir, path) = temp_store();
    let parent = path.parent().expect("parent");
    std::fs::create_dir_all(parent).expect("mkdir");
    std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o755)).expect("widen");
    let mut store = AuthStore::new();
    store.set("https://e/v1", "k").expect("set");

    store.save(&path).expect("save");

    let mode = std::fs::metadata(parent)
        .expect("metadata")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o755, "got {:o}", mode & 0o777);
}

#[cfg(unix)]
#[test]
fn saving_narrows_a_file_whose_mode_was_widened() {
    // A store written before the mode was enforced, or one a user chmod'd, is
    // narrowed on the next save rather than left as found.
    use std::os::unix::fs::PermissionsExt;

    let (_dir, path) = temp_store();
    let mut store = AuthStore::new();
    store.set("https://e/v1", "k").expect("set");
    store.save(&path).expect("first save");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).expect("widen");

    store.save(&path).expect("second save");

    let mode = std::fs::metadata(&path)
        .expect("metadata")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600, "got {:o}", mode & 0o777);
}

#[test]
fn an_endpoint_is_matched_regardless_of_a_trailing_slash_or_case() {
    // Both spellings look right, so a store that disagreed would report "no key"
    // for a config that had just stored one, with no visible cause.
    let mut store = AuthStore::new();
    store
        .set("https://API.Z.AI/api/coding/paas/v4/", "k")
        .expect("set");

    assert_eq!(store.get("https://api.z.ai/api/coding/paas/v4"), Some("k"));
    assert_eq!(store.get("https://api.z.ai/api/coding/paas/v4/"), Some("k"));
    assert_eq!(store.get("https://API.Z.AI/api/coding/paas/v4"), Some("k"));
}

#[test]
fn two_paths_on_one_host_are_separate_credentials() {
    // api.minimax.io serves /v1 and /anthropic/v1, and they can carry different
    // keys. Normalising the path away would hand one endpoint's key to the other.
    let mut store = AuthStore::new();
    store
        .set("https://api.minimax.io/v1", "openai-key")
        .expect("set");
    store
        .set("https://api.minimax.io/anthropic/v1", "anthropic-key")
        .expect("set");

    assert_eq!(store.get("https://api.minimax.io/v1"), Some("openai-key"));
    assert_eq!(
        store.get("https://api.minimax.io/anthropic/v1"),
        Some("anthropic-key")
    );
}

#[test]
fn setting_the_same_endpoint_twice_replaces_the_key() {
    let mut store = AuthStore::new();
    store.set("https://e/v1", "old").expect("set");
    store.set("https://e/v1", "new").expect("set");

    assert_eq!(store.get("https://e/v1"), Some("new"));
    assert_eq!(store.endpoints().len(), 1, "not a second entry");
}

#[test]
fn a_pasted_key_loses_its_surrounding_whitespace() {
    // A key pasted into a terminal routinely arrives with a trailing newline.
    let mut store = AuthStore::new();
    store.set("https://e/v1", "  sk-abc\n").expect("set");

    assert_eq!(store.get("https://e/v1"), Some("sk-abc"));
}

#[test]
fn an_empty_key_is_refused_rather_than_stored() {
    // It would satisfy every "is a key present" check and then fail at the
    // endpoint with a 401 - the confusing-empty-credential failure that `${VAR}`
    // expansion already refuses for the same reason.
    let mut store = AuthStore::new();

    let err = store
        .set("https://e/v1", "   ")
        .expect_err("empty is refused");

    assert!(
        matches!(err, AuthError::EmptyKey(ref e) if e == "https://e/v1"),
        "expected EmptyKey naming the endpoint, got {err:?}"
    );
    assert!(store.is_empty(), "and nothing was stored");
}

#[test]
fn removing_reports_whether_anything_was_held() {
    let mut store = AuthStore::new();
    store.set("https://e/v1", "k").expect("set");

    assert!(store.remove("https://e/v1"), "held a key");
    assert!(!store.remove("https://e/v1"), "no longer holds one");
    assert!(store.is_empty());
}

#[test]
fn removing_normalises_the_endpoint_too() {
    let mut store = AuthStore::new();
    store.set("https://e/v1", "k").expect("set");

    assert!(
        store.remove("https://E/v1/"),
        "the same endpoint, spelled differently"
    );
}

#[test]
fn endpoints_are_listed_in_a_stable_order() {
    // The file is written in this order, so an unstable one would produce a
    // spurious diff on every save.
    let mut store = AuthStore::new();
    for endpoint in ["https://c/v1", "https://a/v1", "https://b/v1"] {
        store.set(endpoint, "k").expect("set");
    }

    assert_eq!(
        store.endpoints(),
        vec!["https://a/v1", "https://b/v1", "https://c/v1"]
    );
}

#[test]
fn debug_prints_endpoints_and_never_keys() {
    // The reason this is hand-written: one `{:?}` anywhere would otherwise emit
    // every credential the user has.
    let mut store = AuthStore::new();
    store.set("https://e/v1", "sk-super-secret").expect("set");

    let rendered = format!("{store:?}");

    assert!(rendered.contains("https://e/v1"), "got {rendered}");
    assert!(
        !rendered.contains("sk-super-secret"),
        "the key reached a log: {rendered}"
    );
}

#[test]
fn the_default_path_the_override_and_a_round_trip_through_it() {
    // `path_from` takes the override rather than reading it, so this needs no
    // `std::env::set_var` - which is `unsafe` in edition 2024 precisely because
    // another thread reading the environment is a data race, and `cargo test`
    // is multi-threaded.
    let platform = path_from(None).expect("this platform has a config dir");
    assert_eq!(platform.file_name().expect("file name"), "auth.toml");
    assert!(
        platform.to_string_lossy().contains("drep"),
        "sits under an application-identity directory: {}",
        platform.display()
    );

    let dir = tempfile::tempdir().expect("tempdir");
    let scratch = dir.path().join("nested").join("auth.toml");

    assert_eq!(
        path_from(Some(scratch.clone().into_os_string())).expect("path"),
        scratch,
        "the override wins over the platform path"
    );

    // `save_default`/`load_default` resolve through `default_path`, so they are
    // exercised against the scratch file by path rather than by variable.
    let mut store = AuthStore::new();
    store.set("https://e/v1", "k-default").expect("set");
    store.save(&scratch).expect("save writes to the override");

    assert!(scratch.exists(), "a file was actually written");
    assert_eq!(
        AuthStore::load(&scratch)
            .expect("reload")
            .get("https://e/v1"),
        Some("k-default"),
        "and it round-trips"
    );
}

#[test]
fn a_key_carrying_a_control_character_is_refused() {
    // Every use of a stored key is an HTTP header value, which cannot carry
    // one. Storing it would defer a guaranteed failure to the first request of
    // the first push, reported as a transport error rather than a bad paste.
    let mut store = AuthStore::new();

    let err = store
        .set("https://e/v1", "sk-abc\u{1b}[0mdef")
        .expect_err("an escape sequence is not a usable key");

    assert!(matches!(err, AuthError::UnusableKey(_)), "got {err:?}");
    assert!(store.is_empty(), "and nothing was stored");
}

#[test]
fn an_interior_newline_is_refused_but_a_trailing_one_is_trimmed() {
    // The difference matters: a pasted key routinely arrives with a trailing
    // newline, which is not a defect. One in the middle is.
    let mut store = AuthStore::new();

    store
        .set("https://e/v1", "sk-fine\n")
        .expect("trailing is trimmed");
    assert_eq!(store.get("https://e/v1"), Some("sk-fine"));

    assert!(
        store.set("https://e/v1", "sk-bro\nken").is_err(),
        "an interior newline cannot be sent"
    );
}

#[test]
fn a_url_path_keeps_its_case_while_the_host_does_not() {
    // Paths are case-sensitive; scheme and host are not. Lowercasing the whole
    // endpoint collapsed `/API/v1` and `/api/v1` onto one entry, which for a
    // host serving both hands one endpoint's key to the other.
    let mut store = AuthStore::new();
    store
        .set("https://Example.COM/API/v1", "upper")
        .expect("set");
    store
        .set("https://example.com/api/v1", "lower")
        .expect("set");

    assert_eq!(
        store.endpoints().len(),
        2,
        "two distinct paths, two entries"
    );
    assert_eq!(store.get("https://EXAMPLE.com/API/v1"), Some("upper"));
    assert_eq!(store.get("https://example.com/api/v1"), Some("lower"));
}

#[test]
fn saving_leaves_no_temporary_beside_the_store() {
    // The write goes through a sibling and a rename. A temporary left behind
    // would sit in the same 0700 directory as the store and hold the same
    // credentials, so "it works" is not the whole requirement.
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("auth.toml");

    let mut store = AuthStore::new();
    store.set("https://api.z.ai/v1", "sk-one").expect("set");
    store.save(&path).expect("first save");
    store.set("https://api.z.ai/v1", "sk-two").expect("set");
    store.save(&path).expect("second save");

    let names: Vec<String> = std::fs::read_dir(dir.path())
        .expect("read_dir")
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();

    assert_eq!(names, vec!["auth.toml".to_string()], "got {names:?}");
    assert_eq!(
        AuthStore::load(&path)
            .expect("loads")
            .get("https://api.z.ai/v1"),
        Some("sk-two"),
        "and the second save is the one that survived"
    );
}

#[test]
fn a_save_that_cannot_be_published_leaves_the_previous_store_readable() {
    // The reason the rename exists. Renaming onto a non-empty directory fails,
    // which stands in for any failure between opening the replacement and
    // publishing it - a crash, a full disk, a serialization error. Truncating
    // the real path in place would have destroyed the old keys before this
    // point, and they are the one thing here that cannot be regenerated.
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("auth.toml");

    let mut store = AuthStore::new();
    store
        .set("https://api.z.ai/v1", "sk-original")
        .expect("set");
    store.save(&path).expect("the first save succeeds");

    // Replace the store with a directory that cannot be renamed over.
    std::fs::remove_file(&path).expect("remove");
    std::fs::create_dir(&path).expect("mkdir");
    std::fs::write(path.join("occupied"), "x").expect("write");

    let mut replacement = AuthStore::new();
    replacement
        .set("https://api.z.ai/v1", "sk-replacement")
        .expect("set");

    assert!(
        replacement.save(&path).is_err(),
        "publishing over a non-empty directory cannot succeed"
    );
    assert!(
        path.join("occupied").exists(),
        "and what was there is untouched"
    );
    assert!(
        !path.with_file_name("auth.toml.drep-tmp").exists(),
        "with no temporary left holding the credentials"
    );
}

#[test]
fn a_scheme_less_endpoints_path_is_case_sensitive_too() {
    // `localhost:11434/v1` is what a user types for a local server, and it has
    // a path like any other endpoint. Lowercasing the whole string because
    // there was no `://` collapsed two endpoints onto one entry and handed one
    // of them the other's key - the case the scheme-carrying branch already
    // refuses.
    let mut store = AuthStore::new();
    store.set("LocalHost:11434/V1", "sk-upper").expect("set");
    store.set("localhost:11434/v1", "sk-lower").expect("set");

    assert_eq!(store.get("localhost:11434/V1"), Some("sk-upper"));
    assert_eq!(store.get("LOCALHOST:11434/v1"), Some("sk-lower"));
}
