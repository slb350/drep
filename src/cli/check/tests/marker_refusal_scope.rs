//! Which repository's policy is consulted, and whose source is at stake.
//!
//! `drep check` reviews the paths it is given, and nothing confines those to the
//! directory the run was rooted in. So "is this repository marked" and "is the
//! source about to be sent marked" are two questions, and the refusal has to
//! answer the second. Both fixtures here put a marked repository somewhere the
//! root probe cannot see it: beside the root, and nested inside it.
//!
//! The other half of the file is the fail-closed arm reaching the gate. The unit
//! test in `config::tests::site` pins `refusal_for` returning
//! `MarkerRootUnresolved`; nothing pinned `check` propagating it rather than
//! reviewing, and every acceptance fixture next door is a git repository, so a
//! swallowed error was invisible.

use super::marker_refusal::{MARKER, check_paths, repo};
use super::support::write_site_policy;
use crate::cli::check;
use crate::test_support::{git_init, git_unresolvable, request_count, server_returning};

/// A marked git repository holding one source file, of its own.
fn marked_repository() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    git_init(dir.path());
    std::fs::write(dir.path().join(MARKER), "").expect("marker");
    std::fs::write(dir.path().join("lib.py"), "x = 1\n").expect("lib.py");
    dir
}

/// A named path outside the run's own repository is still that repository's
/// source.
///
/// An editor plugin or a CI step invoking `drep check <absolute path>` from a
/// fixed working directory reaches this without trying to. Probing `root` alone
/// consulted the unmarked repository's policy and sent the marked one's code.
#[tokio::test]
async fn a_marked_repository_is_refused_when_the_run_is_rooted_outside_it() {
    let (unmarked, server, site) = repo(&[MARKER]).await;
    let marked = marked_repository();

    let exit = check_paths(
        unmarked.path(),
        unmarked.path(),
        &site,
        vec![marked.path().join("lib.py")],
    )
    .await
    .expect("a refusal is a verdict, not a crash");

    assert_eq!(
        exit,
        check::Exit::Unanalyzed,
        "the repository whose source was about to be sent is the one whose policy \
         decides"
    );
    assert_eq!(
        request_count(&server).await,
        0,
        "the marked repository's source left the machine"
    );
}

/// A marked repository checked out inside an unmarked one.
///
/// The walk prunes `.git` and descends into everything beside it, so a bare
/// `drep check` at the outer root reviews the inner checkout's files while only
/// the outer root carries a policy answer.
#[tokio::test]
async fn a_marked_repository_nested_in_an_unmarked_one_is_refused() {
    let (outer, server, site) = repo(&[MARKER]).await;
    let inner = outer.path().join("vendored-service");
    std::fs::create_dir(&inner).expect("nested checkout");
    git_init(&inner);
    std::fs::write(inner.join(MARKER), "").expect("marker");
    std::fs::write(inner.join("lib.py"), "y = 2\n").expect("lib.py");

    let exit = check_paths(outer.path(), outer.path(), &site, Vec::new())
        .await
        .expect("a refusal is a verdict, not a crash");

    assert_eq!(exit, check::Exit::Unanalyzed);
    assert_eq!(request_count(&server).await, 0);
}

/// The discriminating half: an unmarked repository beside an unmarked root still
/// reviews.
///
/// Without it, "refuse whenever a named path sits outside `root`" passes both
/// tests above.
#[tokio::test]
async fn an_unmarked_repository_outside_the_root_still_reviews() {
    let (unmarked, server, site) = repo(&[MARKER]).await;
    let beside = tempfile::tempdir().expect("tempdir");
    git_init(beside.path());
    std::fs::write(beside.path().join("lib.py"), "x = 1\n").expect("lib.py");

    let exit = check_paths(
        unmarked.path(),
        unmarked.path(),
        &site,
        vec![beside.path().join("lib.py")],
    )
    .await
    .expect("neither repository is marked");

    assert_eq!(exit, check::Exit::Clean);
    assert_eq!(request_count(&server).await, 1);
}

/// A policy naming markers that cannot be evaluated stops the gate.
///
/// "Cannot be evaluated" becoming "evaluates to allowed" is the whole defect
/// class this layer exists to refuse, and the gate is where it would be silent:
/// `doctor` deliberately describes the same error instead of failing, so a
/// refusal swallowed here would look exactly like an ordinary clean run.
#[tokio::test]
async fn a_policy_that_cannot_be_evaluated_stops_the_check_rather_than_reviewing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let server = server_returning(&[r#"{"issues": []}"#]).await;
    crate::test_support::write_drep_toml(dir.path(), &format!("{}/v1", server.uri()));
    std::fs::write(dir.path().join("lib.py"), "x = 1\n").expect("lib.py");
    let site = write_site_policy(dir.path(), &[MARKER]);
    git_unresolvable(dir.path());

    let err = check_paths(
        dir.path(),
        dir.path(),
        &site,
        vec![dir.path().join("lib.py")],
    )
    .await
    .expect_err("a policy that could not be evaluated is not a policy that permits");

    let message = format!("{err:#}");
    assert!(
        message.contains(&site.display().to_string()),
        "the operator has to be told which policy could not be evaluated; got {message}"
    );
    assert_eq!(
        request_count(&server).await,
        0,
        "and the source must not have been sent while it could not be; got {message}"
    );
}

/// The same directory, with the policy naming no markers, reviews normally.
///
/// This is what keeps the test above about the marker policy rather than about
/// the fixture being unusable, and it is also the property that keeps an
/// unaffected machine free of the new failure mode entirely: no markers, no git
/// query, no way to fail closed.
#[tokio::test]
async fn a_policy_naming_no_markers_reviews_outside_a_repository() {
    let dir = tempfile::tempdir().expect("tempdir");
    let server = server_returning(&[r#"{"issues": []}"#]).await;
    crate::test_support::write_drep_toml(dir.path(), &format!("{}/v1", server.uri()));
    std::fs::write(dir.path().join("lib.py"), "x = 1\n").expect("lib.py");
    let site = dir.path().join("site.toml");
    std::fs::write(&site, "max_concurrent_ceiling = 4\n").expect("site policy");
    git_unresolvable(dir.path());

    let exit = check_paths(
        dir.path(),
        dir.path(),
        &site,
        vec![dir.path().join("lib.py")],
    )
    .await
    .expect("a policy naming no markers needs no repository");

    assert_eq!(exit, check::Exit::Clean);
    assert_eq!(request_count(&server).await, 1);
}

/// Every marker probe is against a repository root, whichever directory asked.
///
/// The scope fix probes the directory of each file, and a file in a subdirectory
/// of a marked repository must resolve to that repository's root rather than to
/// its own directory - the same rule `a_check_from_a_subdirectory_of_a_marked_
/// repository_is_refused` pins for `root`.
#[tokio::test]
async fn a_file_deep_inside_a_marked_repository_is_refused() {
    let (unmarked, server, site) = repo(&[MARKER]).await;
    let marked = marked_repository();
    let deep = marked.path().join("service/handlers");
    std::fs::create_dir_all(&deep).expect("subdirectories");
    std::fs::write(deep.join("lib.py"), "z = 3\n").expect("lib.py");

    let exit = check_paths(
        unmarked.path(),
        unmarked.path(),
        &site,
        vec![deep.join("lib.py")],
    )
    .await
    .expect("a refusal is a verdict, not a crash");

    assert_eq!(exit, check::Exit::Unanalyzed);
    assert_eq!(request_count(&server).await, 0);
}

/// One `SitePolicyRefused` entry per file, across repositories.
///
/// The refusal reports the files drep was asked about, and asking about files in
/// two repositories is still one refused run: the marked one decides, and the
/// files from the unmarked one were never reviewed either.
#[tokio::test]
async fn files_from_an_unmarked_repository_are_refused_alongside_the_marked_one() {
    let (unmarked, server, site) = repo(&[MARKER]).await;
    let marked = marked_repository();

    let exit = check_paths(
        unmarked.path(),
        unmarked.path(),
        &site,
        vec![unmarked.path().join("lib.py"), marked.path().join("lib.py")],
    )
    .await
    .expect("a refusal is a verdict, not a crash");

    assert_eq!(
        exit,
        check::Exit::Unanalyzed,
        "one run reviews one work set, and half of it is not allowed to be sent"
    );
    assert_eq!(request_count(&server).await, 0);
}

/// Nothing here should depend on the temporary directory's own ancestry.
///
/// `git_unresolvable` is what makes that true, and this is the assertion that
/// says so: on a machine whose `TMPDIR` sits inside a checkout, a plain temporary
/// directory resolves a repository root and the two fail-closed tests above would
/// pass for the wrong reason.
#[tokio::test]
async fn the_unresolvable_fixture_really_has_no_repository_root() {
    let dir = tempfile::tempdir().expect("tempdir");
    git_unresolvable(dir.path());

    let resolved = crate::diff::repository_root(dir.path()).await;

    assert!(
        resolved.is_err(),
        "the fixture has to deny git a root wherever the temporary directory \
         lives; got {resolved:?}"
    );
}
