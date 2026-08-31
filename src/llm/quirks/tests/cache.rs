//! The cache file, and when it is trusted.

use std::path::{Path, PathBuf};

use super::super::{Cached, QuirksSource, Registry};
use super::{Canned, DOCUMENT};

const KIMI: &str = "https://api.kimi.com/coding/v1";
const WEEK: u64 = 7 * 24 * 60 * 60;

/// A cache path inside `dir`, never the developer's real one.
fn cache_path(dir: &tempfile::TempDir) -> PathBuf {
    dir.path().join("model-quirks.toml")
}

/// Write a registry stamped `fetched_at` to `path`.
fn seed(path: &Path, fetched_at: u64) {
    Registry::distil(DOCUMENT, fetched_at)
        .expect("distils")
        .save(path)
        .expect("saves");
}

#[tokio::test]
async fn a_fresh_cache_is_used_without_fetching() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = cache_path(&dir);
    seed(&path, 1_000);

    let fetcher = Canned::offline();
    let registry = Cached::at(Some(path), &fetcher, 1_000 + WEEK)
        .registry()
        .await
        .expect("the cache answers");

    assert_eq!(fetcher.calls.get(), 0, "a fresh cache costs no request");
    assert!(registry.facts(KIMI, "k3").is_some());
}

#[tokio::test]
async fn a_cache_one_second_past_the_week_is_refetched() {
    // The boundary itself, because `>` and `>=` are one character apart and
    // both look right.
    let dir = tempfile::tempdir().expect("tempdir");
    let path = cache_path(&dir);
    seed(&path, 1_000);

    let fetcher = Canned::serving(DOCUMENT);
    Cached::at(Some(path), &fetcher, 1_000 + WEEK + 1)
        .registry()
        .await
        .expect("the fetch answers");

    assert_eq!(fetcher.calls.get(), 1);
}

#[tokio::test]
async fn a_missing_cache_is_fetched_and_written() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = cache_path(&dir);

    let fetcher = Canned::serving(DOCUMENT);
    let registry = Cached::at(Some(path.clone()), &fetcher, 5_000)
        .registry()
        .await
        .expect("the fetch answers");

    assert_eq!(fetcher.calls.get(), 1);
    assert_eq!(
        Registry::load(&path).as_ref(),
        Some(&registry),
        "what was fetched is what was cached"
    );
}

#[tokio::test]
async fn what_was_written_is_read_back_without_a_second_fetch() {
    // The cache is only worth writing if the next run trusts it, which no test
    // of `save` alone can show.
    let dir = tempfile::tempdir().expect("tempdir");
    let path = cache_path(&dir);

    let first = Canned::serving(DOCUMENT);
    Cached::at(Some(path.clone()), &first, 5_000)
        .registry()
        .await
        .expect("the fetch answers");

    let second = Canned::offline();
    let registry = Cached::at(Some(path), &second, 5_001)
        .registry()
        .await
        .expect("the cache answers");

    assert_eq!(second.calls.get(), 0);
    assert_eq!(
        registry.facts(KIMI, "k3").and_then(|f| f.output_limit),
        Some(131_072),
        "the facts survive the round trip through TOML"
    );
}

#[tokio::test]
async fn an_unreadable_cache_is_fetched() {
    // A directory where the file should be: `read_to_string` fails, and the
    // caller must treat that as "no cache" rather than as an error to report.
    let dir = tempfile::tempdir().expect("tempdir");
    let path = cache_path(&dir);
    std::fs::create_dir(&path).expect("mkdir");

    let fetcher = Canned::serving(DOCUMENT);
    let registry = Cached::at(Some(path), &fetcher, 5_000)
        .registry()
        .await
        .expect("the fetch answers");

    assert_eq!(fetcher.calls.get(), 1);
    assert!(registry.facts(KIMI, "k3").is_some());
}

#[tokio::test]
async fn an_unparseable_cache_is_fetched() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = cache_path(&dir);
    std::fs::write(&path, "this is not toml {{{").expect("write");

    let fetcher = Canned::serving(DOCUMENT);
    Cached::at(Some(path), &fetcher, 5_000)
        .registry()
        .await
        .expect("the fetch answers");

    assert_eq!(fetcher.calls.get(), 1);
}

#[tokio::test]
async fn a_cache_of_the_wrong_shape_is_fetched_too() {
    // Valid TOML, wrong contents - a truncated write, or a file from a future
    // version. Parsing it as an empty registry would silently answer every
    // lookup with "unknown" and never refetch.
    let dir = tempfile::tempdir().expect("tempdir");
    let path = cache_path(&dir);
    std::fs::write(&path, "unrelated = true\n").expect("write");

    let fetcher = Canned::serving(DOCUMENT);
    Cached::at(Some(path), &fetcher, 5_000)
        .registry()
        .await
        .expect("the fetch answers");

    assert_eq!(fetcher.calls.get(), 1);
}

#[tokio::test]
async fn a_failed_fetch_with_no_cache_reports_why() {
    // Non-fatal to `drep init` - the wizard reports this and carries on - but
    // it has to be an error rather than an empty registry, or the caller cannot
    // tell "models.dev says nothing about your model" from "models.dev was
    // unreachable".
    let dir = tempfile::tempdir().expect("tempdir");

    let err = Cached::at(Some(cache_path(&dir)), &Canned::offline(), 5_000)
        .registry()
        .await
        .expect_err("nothing to answer with");

    assert!(err.to_string().contains("could not reach"), "got {err}");
}

#[tokio::test]
async fn a_failed_fetch_falls_back_to_a_stale_cache() {
    // What a model accepts does not change once it has shipped, so a week-old
    // copy is worth more than nothing. Refusing it would make a user offline
    // for eight days strictly worse off than one offline for six.
    let dir = tempfile::tempdir().expect("tempdir");
    let path = cache_path(&dir);
    seed(&path, 1_000);

    let registry = Cached::at(Some(path), &Canned::offline(), 1_000 + 30 * WEEK)
        .registry()
        .await
        .expect("the stale cache answers");

    assert_eq!(
        registry.facts(KIMI, "k3").and_then(|f| f.output_limit),
        Some(131_072)
    );
}

#[tokio::test]
async fn a_document_that_will_not_distil_leaves_the_cache_alone() {
    // Caching a document drep could not read would make every later run answer
    // "unknown" from disk for a week.
    let dir = tempfile::tempdir().expect("tempdir");
    let path = cache_path(&dir);

    Cached::at(
        Some(path.clone()),
        &Canned::serving("<html>502</html>"),
        5_000,
    )
    .registry()
    .await
    .expect_err("the document is unreadable");

    assert!(!path.exists(), "nothing should have been written");
}

#[tokio::test]
async fn no_cache_path_fetches_every_time() {
    // A platform with no config directory. Slower, never wrong.
    let fetcher = Canned::serving(DOCUMENT);
    let source = Cached::at(None, &fetcher, 5_000);

    source.registry().await.expect("first");
    source.registry().await.expect("second");

    assert_eq!(fetcher.calls.get(), 2);
}

#[tokio::test]
async fn a_cache_that_cannot_be_written_still_answers() {
    // The registry in hand is the same either way, so a read-only config
    // directory is a slower run rather than a failed one.
    let dir = tempfile::tempdir().expect("tempdir");
    let blocked = dir.path().join("a-file");
    std::fs::write(&blocked, "not a directory").expect("write");

    let registry = Cached::at(
        Some(blocked.join("model-quirks.toml")),
        &Canned::serving(DOCUMENT),
        5_000,
    )
    .registry()
    .await
    .expect("the fetch still answers");

    assert!(registry.facts(KIMI, "k3").is_some());
}

#[test]
fn a_clock_that_moved_backwards_reads_as_fresh() {
    // `now - fetched_at` on unsigned integers wraps into an enormous age, which
    // would refetch on every run for as long as the clock was behind.
    let registry = Registry::distil(DOCUMENT, 10_000).expect("distils");

    assert!(!registry.is_stale(0));
    assert!(!registry.is_stale(9_999));
}

#[test]
fn a_cache_exactly_a_week_old_is_still_fresh() {
    let registry = Registry::distil(DOCUMENT, 1_000).expect("distils");

    assert!(!registry.is_stale(1_000 + WEEK));
    assert!(registry.is_stale(1_000 + WEEK + 1));
}

#[test]
fn saving_reports_a_directory_it_cannot_create() {
    let dir = tempfile::tempdir().expect("tempdir");
    let blocked = dir.path().join("a-file");
    std::fs::write(&blocked, "not a directory").expect("write");

    let err = Registry::distil(DOCUMENT, 0)
        .expect("distils")
        .save(&blocked.join("nested").join("model-quirks.toml"))
        .expect_err("the parent cannot be created");

    assert!(err.to_string().contains("could not write"), "got {err}");
}

#[cfg(unix)]
#[test]
fn saving_never_writes_through_a_predictable_temporary_symlink() {
    use std::os::unix::fs::symlink;

    let dir = tempfile::tempdir().expect("tempdir");
    let path = cache_path(&dir);
    let predictable = path.with_extension("toml.tmp");
    let outside = dir.path().join("outside.txt");
    std::fs::write(&outside, "keep me").expect("outside seed");
    symlink(&outside, &predictable).expect("planted sibling symlink");

    Registry::distil(DOCUMENT, 0)
        .expect("distils")
        .save(&path)
        .expect("saves without using the planted name");

    assert_eq!(
        std::fs::read_to_string(&outside).expect("outside body"),
        "keep me",
        "cache publication must not write through an attacker-planted sibling symlink"
    );
    assert!(path.is_file(), "the cache must still be published");
}

#[test]
fn the_cache_keeps_only_the_two_facts_drep_reads() {
    // The reason the document is distilled rather than stored: models.dev is
    // ~4 MB against the 600 KB this writes, and drep reads a boolean and an
    // integer per model. Everything else the vendors publish is dropped.
    //
    // Asserted as "the ignored fields are gone" rather than as a size ratio.
    // A ratio over a cut-down fixture measures how verbose the fixture is -
    // adding one model to it moved the number - while what distillation has to
    // guarantee is which fields survive.
    let body = toml::to_string(&Registry::distil(DOCUMENT, 0).expect("distils")).expect("toml");

    for dropped in ["context", "reasoning", "\"name\"", "Kimi K3"] {
        assert!(
            !body.contains(dropped),
            "`{dropped}` is not a fact drep reads, so it must not reach the cache:\n{body}"
        );
    }
    assert!(
        body.contains("output_limit") && body.contains("temperature"),
        "and the two that are read must survive:\n{body}"
    );
    assert!(
        body.len() < DOCUMENT.len(),
        "distilling must not grow the document: {} bytes from {}",
        body.len(),
        DOCUMENT.len()
    );
}

#[cfg(unix)]
#[test]
fn a_directory_the_cache_creates_is_private() {
    // The cache shares a directory with `auth.toml`. Creating it here without
    // narrowing it would leave the credential store's own directory
    // world-readable whenever `drep init` happened to cache first.
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().expect("tempdir");
    let nested = dir.path().join("drep");
    Registry::distil(DOCUMENT, 0)
        .expect("distils")
        .save(&nested.join("model-quirks.toml"))
        .expect("saves");

    let mode = std::fs::metadata(&nested)
        .expect("metadata")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o700, "got {:o}", mode & 0o777);
}

#[test]
fn the_override_relocates_the_cache_and_the_default_sits_beside_auth_toml() {
    // Read as a parameter rather than from the process, because
    // `std::env::set_var` is `unsafe` in edition 2024 and `cargo test` is
    // multi-threaded - the same split `auth::path_from` exists for.
    use super::super::path_from;

    assert_eq!(
        path_from(Some(std::ffi::OsString::from("/tmp/elsewhere.toml"))),
        Some(PathBuf::from("/tmp/elsewhere.toml"))
    );

    let default = path_from(None).expect("this platform has a config directory");
    assert_eq!(
        default.file_name().and_then(|n| n.to_str()),
        Some("model-quirks.toml")
    );
    assert_eq!(
        default.parent(),
        crate::auth::path_from(None)
            .expect("this platform has a config directory")
            .parent(),
        "the cache and the credential store are siblings"
    );
}

#[test]
fn the_stamp_on_a_new_registry_is_the_wall_clock() {
    // `unix_now` stamps every registry drep writes and is the `now` every
    // staleness check is measured against, so a constant would make the cache
    // either permanently fresh or permanently stale - and both look like a
    // working cache from the outside.
    let now = super::super::unix_now();

    assert!(now > 1_735_689_600, "before 2025: {now}");
    assert!(now < 4_102_444_800, "after 2100: {now}");
}

#[test]
fn the_default_path_reads_the_override_variable() {
    // The variable is read, never written: `std::env::set_var` is `unsafe` in
    // edition 2024 and `cargo test` is multi-threaded, so this asserts against
    // whichever state the process is already in.
    use super::super::{PATH_VAR, default_path, path_from};

    match std::env::var_os(PATH_VAR) {
        Some(overridden) => assert_eq!(default_path(), Some(PathBuf::from(overridden))),
        None => {
            let platform = default_path().expect("this platform has a config directory");
            assert_eq!(
                platform.file_name().and_then(|name| name.to_str()),
                Some("model-quirks.toml")
            );
            assert_eq!(platform, path_from(None).expect("the same platform path"));
        }
    }
}
