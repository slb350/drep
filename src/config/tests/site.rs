//! The machine-level site policy layer: the loader, its fatality contract, its
//! path resolution and the concurrency ceiling.
//!
//! A separate file from `fields` and `providers` because the topic is a
//! different file with a different grammar and a different error type. The
//! through-line of every test here is that a policy which fails to load must
//! never read as a policy that found nothing to complain about.

use std::ffi::OsString;

use super::support::{write_config, write_site};
use crate::config::site::{self, SiteConfig, SiteConfigError};
use crate::config::{Config, LlmConfig, load};

/// A config whose entries carry only the enablement and concurrency the test is
/// about.
fn config_of(entries: &[(bool, usize)]) -> Config {
    Config {
        llm: entries
            .iter()
            .map(|&(enabled, max_concurrent)| LlmConfig {
                enabled,
                max_concurrent,
                ..LlmConfig::default()
            })
            .collect(),
        ..Config::default()
    }
}

/// A site policy with the given ceiling and no markers.
fn ceiling_of(ceiling: usize) -> SiteConfig {
    SiteConfig {
        max_concurrent_ceiling: Some(ceiling),
        ..SiteConfig::default()
    }
}

// ---- the loader ----

/// A machine with no policy file is the normal state, not a broken one.
#[test]
fn a_missing_site_file_is_no_policy_rather_than_an_error() {
    let temp = tempfile::tempdir().expect("tempdir");

    let loaded = site::load(&temp.path().join("site.toml"));

    assert!(
        matches!(loaded, Ok(None)),
        "most machines have no policy file, and refusing to run on them would \
         make the layer unshippable; got {loaded:?}"
    );
}

/// The single most important behaviour in the layer.
///
/// A policy file that silently fails to load reports as compliance: the run
/// goes ahead unconstrained and nothing in its output says the policy was never
/// applied.
#[test]
fn an_unparseable_site_file_is_fatal_rather_than_treated_as_absent() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = write_site(&temp, "this is not toml at all\n");

    let err = site::load(&path).expect_err("a policy that cannot be parsed is not enforced");

    assert!(matches!(err, SiteConfigError::Parse(_, _)), "got {err:?}");
    assert!(
        err.to_string().contains(&path.display().to_string()),
        "the message has to name the file an operator must fix; got {err}"
    );
}

/// The other half of the same rule, for an I/O failure rather than a grammar
/// one.
///
/// A directory at the site path is the portable way to make the read fail
/// without being `NotFound`. The assertion is deliberately on "not absent"
/// rather than on an `ErrorKind`, because macOS and Linux disagree about which
/// error reading a directory produces.
#[test]
fn an_unreadable_site_file_is_fatal_rather_than_treated_as_absent() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("site.toml");
    std::fs::create_dir(&path).expect("directory in place of the file");

    let loaded = site::load(&path);

    assert!(
        loaded.is_err(),
        "a loader that maps every I/O failure to `Ok(None)` reports an \
         unenforced policy as no policy; got {loaded:?}"
    );
}

/// A misspelled policy key is a policy that does nothing, so it is rejected.
#[test]
fn unknown_keys_are_rejected_so_a_typo_in_a_policy_file_is_loud() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = write_site(&temp, "refuse_marker = [\".drep-no-llm\"]\n");

    let err = site::load(&path).expect_err("a key drep does not read is not a policy");

    assert!(
        err.to_string().contains("refuse_marker"),
        "the message has to name the key that was ignored; got {err}"
    );
}

/// Requirement 7, and the reason [`SiteConfig`] may derive `Debug`: there is no
/// credential and no provider it could print.
#[test]
fn a_site_file_cannot_declare_a_provider_or_a_credential() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = write_site(
        &temp,
        "[[llm]]\nendpoint = \"https://gateway.example/v1\"\napi_key = \"literal-secret\"\n",
    );

    let err = site::load(&path).expect_err("policy is not a place a provider can be declared");

    assert!(matches!(err, SiteConfigError::Parse(_, _)), "got {err:?}");
    assert!(
        !err.to_string().contains("literal-secret"),
        "and rejecting it must not quote it back; got {err}"
    );
}

/// The clamp runs after `config::validate`, so a ceiling of zero would rebuild
/// the exact hang `ConfigError::ZeroConcurrency` exists to prevent - a
/// semaphore with no permits, waited on forever with no message.
#[test]
fn a_zero_ceiling_is_rejected_rather_than_clamping_every_provider_to_a_hanging_semaphore() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = write_site(&temp, "max_concurrent_ceiling = 0\n");

    let err = site::load(&path).expect_err("a ceiling of zero authorizes nothing");

    assert!(
        matches!(err, SiteConfigError::ZeroConcurrencyCeiling(_)),
        "got {err:?}"
    );
}

// ---- `refuse_markers`: parsed and validated here, enforced by feature C ----

#[test]
fn refuse_markers_round_trip_as_a_list_of_filenames() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = write_site(
        &temp,
        "refuse_markers = [\".drep-no-llm\", \"NO_SEMANTIC_REVIEW\"]\n",
    );

    let site = site::load(&path)
        .expect("a list of filenames is the documented shape")
        .expect("the file exists");

    assert_eq!(site.refuse_markers, [".drep-no-llm", "NO_SEMANTIC_REVIEW"]);
}

#[test]
fn a_site_file_that_names_no_markers_carries_an_empty_list() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = write_site(&temp, "max_concurrent_ceiling = 4\n");

    let site = site::load(&path)
        .expect("a policy may set a ceiling and no markers")
        .expect("the file exists");

    assert!(
        site.refuse_markers.is_empty(),
        "an absent list is empty, not an error; got {:?}",
        site.refuse_markers
    );
}

/// A marker that is not a filename matches no file, so the policy it declares
/// refuses nothing - the silent no-op requirement 3 exists to refuse, one field
/// down.
#[test]
fn a_refuse_marker_that_is_not_a_filename_is_rejected() {
    for body in [
        "refuse_markers = [\"\"]\n",
        "refuse_markers = [\"policy/.drep-no-llm\"]\n",
        "refuse_markers = [\"..\"]\n",
    ] {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = write_site(&temp, body);

        let err = site::load(&path).expect_err("a marker that cannot name a file refuses nothing");

        assert!(
            matches!(err, SiteConfigError::UnusableRefuseMarker { .. }),
            "for {body:?}, got {err:?}"
        );
    }
}

// ---- path resolution ----

/// The whole point of the layer: a file the policed developer can edit without
/// privilege is not a policy file.
#[test]
fn the_default_site_path_is_machine_wide_rather_than_per_user() {
    let path = site::path_from(None);

    #[cfg(target_os = "macos")]
    assert_eq!(
        path,
        std::path::Path::new("/Library/Application Support/drep/site.toml")
    );
    #[cfg(not(target_os = "macos"))]
    assert_eq!(path, std::path::Path::new("/etc/drep/site.toml"));

    if let Some(dirs) = directories::ProjectDirs::from("dev", "slb350", "drep") {
        assert!(
            !path.starts_with(dirs.config_dir()),
            "routing this through `ProjectDirs` for consistency with auth.toml \
             would put policy in a directory its subject owns; got {}",
            path.display()
        );
    }
}

#[test]
fn the_site_path_comes_from_the_environment_when_set() {
    let path = site::path_from(Some(OsString::from("/tmp/drep-policy/site.toml")));

    assert_eq!(path, std::path::Path::new("/tmp/drep-policy/site.toml"));
}

/// `DREP_SITE_CONFIG=` naming nothing must not switch policy off: a set-but-empty
/// variable silently disabling enforcement is the same defect class as a file
/// that fails to load.
#[test]
fn an_empty_override_falls_back_to_the_machine_path_rather_than_disabling_policy() {
    assert_eq!(
        site::path_from(Some(OsString::new())),
        site::path_from(None)
    );
}

// ---- the ceiling ----

#[test]
fn the_ceiling_lowers_a_higher_max_concurrent_and_leaves_a_lower_one_alone() {
    let mut config = config_of(&[(true, 8), (true, 1)]);

    ceiling_of(4).apply(&mut config);

    assert_eq!(
        (config.llm[0].max_concurrent, config.llm[1].max_concurrent),
        (4, 1),
        "an `apply` that assigns the ceiling instead of taking a minimum \
         silently raises a repository that had deliberately lowered itself"
    );
}

/// The ceiling governs the *effective* value, so an entry that never wrote the
/// field is clamped too. Skipping the defaulted ones would let a repository
/// raise its own concurrency by deleting a line.
#[test]
fn the_ceiling_clamps_an_entry_that_never_named_max_concurrent() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = write_config(
        &temp,
        "[[llm]]\nendpoint = \"http://localhost:11434/v1\"\nmodel = \"m\"\n",
    );
    let mut config = load(&path).expect("load");
    assert_eq!(
        config.llm[0].max_concurrent,
        LlmConfig::default().max_concurrent,
        "the fixture has to arrive at the default for the clamp below to mean anything"
    );

    ceiling_of(2).apply(&mut config);

    assert_eq!(config.llm[0].max_concurrent, 2);
}

/// Stops the two tests above from passing for a reason unrelated to the ceiling.
#[test]
fn an_absent_ceiling_leaves_every_provider_untouched() {
    let mut config = config_of(&[(true, 8), (true, 1)]);

    SiteConfig::default().apply(&mut config);

    assert_eq!(
        (config.llm[0].max_concurrent, config.llm[1].max_concurrent),
        (8, 1)
    );
}

/// Disabled entries are inert in every pass over the provider list, and this one
/// is no exception: `doctor` would otherwise report a clamp on a provider drep
/// never contacts.
#[test]
fn a_disabled_entry_is_left_inert_by_the_ceiling() {
    let mut config = config_of(&[(false, 8), (true, 8)]);

    ceiling_of(4).apply(&mut config);

    assert_eq!(
        (config.llm[0].max_concurrent, config.llm[1].max_concurrent),
        (8, 4)
    );
}

/// The invariant the rejected zero ceiling and the rejected zero
/// `max_concurrent` jointly hold: clamping never produces a provider with no
/// permits.
#[test]
fn clamping_can_never_produce_a_zero_permit_provider() {
    for ceiling in 1..=4usize {
        for requested in 1..=8usize {
            let clamped = ceiling_of(ceiling).clamp_concurrency(requested);
            assert!(
                clamped >= 1,
                "ceiling {ceiling} against {requested} produced {clamped}"
            );
            assert!(clamped <= requested, "the ceiling only ever lowers");
        }
    }
}

/// An unaffected machine gains neither a git spawn nor a new failure mode.
///
/// Real proxy for "no git was asked": the directory is not a repository, so a
/// probe that failed to short circuit would return `MarkerRootUnresolved` here.
/// That is the whole reason `drep check` outside a repository keeps working on
/// every machine that installed no marker policy.
#[tokio::test]
async fn no_configured_markers_never_asks_git() {
    let temp = tempfile::tempdir().expect("tempdir");

    let refusal = SiteConfig::default()
        .refusal_for(temp.path(), &temp.path().join("site.toml"))
        .await
        .expect("a policy naming no markers evaluates without git");

    assert!(refusal.is_none());
}

/// A policy that cannot be evaluated must not evaluate to "allowed".
///
/// Markers configured, no repository to resolve them against. Returning
/// `Ok(None)` here would be the unenforced policy reported as compliance that
/// every message in this module refuses - and it would be silent, because the run
/// that followed would look exactly like an ordinary clean one.
#[tokio::test]
async fn configured_markers_outside_a_git_repository_fail_closed() {
    let temp = tempfile::tempdir().expect("tempdir");
    let policy = write_site(&temp, "refuse_markers = [\".drep-no-llm\"]\n");
    let site = site::load(&policy)
        .expect("the policy loads")
        .expect("the policy is present");
    // Not the `TempDir` itself: on a developer machine the temporary directory
    // could sit inside a repository, and then git would answer.
    let outside = std::path::Path::new("/");

    let err = site
        .refusal_for(outside, &policy)
        .await
        .expect_err("a policy that cannot be evaluated is not a policy that permits");

    assert!(matches!(err, SiteConfigError::MarkerRootUnresolved { .. }));
    let message = err.to_string();
    assert!(
        message.contains(&policy.display().to_string()),
        "names the policy: {message}"
    );
    assert!(
        message.contains("refuses to run"),
        "and states the consequence: {message}"
    );
}

/// The marker found is reported with the repository root it was found at, not as
/// the bare filename the policy wrote.
///
/// A developer seeing only `.drep-no-llm` in a monorepo of worktrees cannot tell
/// which checkout answered.
#[tokio::test]
async fn a_found_marker_is_reported_at_the_repository_root() {
    let temp = tempfile::tempdir().expect("tempdir");
    crate::test_support::git_init(temp.path());
    let policy = write_site(&temp, "refuse_markers = [\".drep-no-llm\"]\n");
    let site = site::load(&policy).expect("loads").expect("present");
    std::fs::write(temp.path().join(".drep-no-llm"), "").expect("marker");

    let refusal = site
        .refusal_for(temp.path(), &policy)
        .await
        .expect("evaluating the policy")
        .expect("the marker is present");

    assert_eq!(
        refusal.marker.file_name().and_then(|n| n.to_str()),
        Some(".drep-no-llm")
    );
    assert!(refusal.marker.is_absolute(), "got {:?}", refusal.marker);
    assert_eq!(refusal.policy, policy);
}
