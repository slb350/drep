//! The machine-level site policy layer above `drep.toml`.
//!
//! `drep.toml` is a repository file, and `drep init` adds it to `.gitignore` by
//! default - so it is per-developer scratch, and a control written there is
//! opt-in. Opt-in means off for the person who most needs it. This layer sits
//! above it: a repository checkout can tighten what the site allows, never
//! loosen it.
//!
//! Three rules carry the whole module.
//!
//! - **A missing file is no policy, and is not an error.** Most machines have
//!   none. [`load`] returns `Option<SiteConfig>` so that is structural rather
//!   than a convention a caller could get backwards.
//! - **A file that exists and cannot be loaded is fatal.** A policy that
//!   silently fails to load is worse than no policy at all, because the
//!   unconstrained run that follows reports as compliance. Every
//!   [`SiteConfigError`] says so in its own message.
//! - **The file is not per-user, and the process it constrains cannot move it.**
//!   The location is a system path rather than the `ProjectDirs` directory holding
//!   `auth.toml` and the response cache, because a policy file the policed
//!   developer can edit without privilege is not a policy file. [`PATH_VAR`] names
//!   the file only on a machine where none is installed, for the same reason: an
//!   override that could displace an installed policy would be one `export` away
//!   from switching it off. There is no `${VAR}` expansion in the file either - a
//!   policy that takes its values from the environment of the process it
//!   constrains constrains nothing.
//!
//! The layering is applied by the caller, after [`super::load`] returns, which
//! is what keeps [`super::ConfigError`] a statement about `drep.toml` alone.

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

use futures::StreamExt;
use serde::Deserialize;
use thiserror::Error;

use super::Config;

/// The environment variable that relocates the policy file.
pub const PATH_VAR: &str = "DREP_SITE_CONFIG";

/// How many repository roots resolve at once.
///
/// Four, matching `check::deterministic`'s `TOOL_PROCESS_CONCURRENCY`, because it
/// bounds the same resource: short-lived child processes on a developer machine
/// that is probably also compiling. Its own constant rather than a shared one,
/// since the two fan-outs spawn different programs and would be tuned apart.
const ROOT_RESOLUTION_CONCURRENCY: usize = 4;

/// The machine-wide policy path, per platform.
///
/// Deliberately not under `directories::ProjectDirs`, where `auth.toml` and the
/// cache live: those are the user's own state and belong in the user's own
/// directory, while this file is the thing the user is not supposed to be able
/// to edit. The system path uses the plain `drep` name rather than drep's
/// `dev.slb350.drep` identity triple because an administrator installs it by
/// hand, and a reverse-DNS directory under `/etc` is a path nobody can type.
///
/// A `const` with two `cfg` arms rather than a `cfg`'d pair of functions: a
/// function body that is not compiled on this platform is a mutation the test
/// suite can never detect, which `auth::restrict` records. A const is not a
/// mutation target at all.
#[cfg(target_os = "macos")]
const MACHINE_PATH: &str = "/Library/Application Support/drep/site.toml";
#[cfg(not(target_os = "macos"))]
const MACHINE_PATH: &str = "/etc/drep/site.toml";

/// The policy path: the machine-wide file, or [`PATH_VAR`] when there is none.
pub fn default_path() -> PathBuf {
    path_from(std::env::var_os(PATH_VAR), machine_path())
}

/// The machine-wide path this platform installs policy at.
///
/// A function so `default_path` and its test read one thing rather than each
/// restating the literal, and so the two `cfg` arms above have a single reader.
pub fn machine_path() -> &'static Path {
    Path::new(MACHINE_PATH)
}

/// [`default_path`] with the override and the machine path supplied rather than
/// read.
///
/// Split out for the reason `auth::path_from` is: `std::env::set_var` is
/// `unsafe` in edition 2024 because another thread reading the environment is a
/// data race, and `cargo test` is multi-threaded, so the override has to be
/// suppliable to be testable at all. The machine path joins it because a test
/// cannot write to `/etc` either.
///
/// **The override cannot displace an installed policy.** If a file exists at
/// `machine`, that file is the policy and the variable is ignored. Otherwise the
/// whole layer is one `export` away from off: the developer `refuse_markers`
/// constrains points the variable at an empty file, the marker list is empty, the
/// probe short-circuits before git is even spawned, and the run sends the
/// repository's source and exits 0. `ConfigError::SiteOnlyField` refuses that
/// field in `drep.toml` because a refusal a developer can delete is not one, and a
/// per-process override is a way to delete it. The precedence is not silent
/// either: `drep doctor` names the file in effect, so an administrator who moved
/// the policy and left the old one behind can see which one answered.
///
/// So the variable names the policy on a machine that installed none - an
/// installation that keeps the file elsewhere, and every test in this suite, which
/// must not read whatever this machine happens to hold. An **empty** value falls
/// back to the machine path rather than being honoured as a file that does not
/// exist, because `DREP_SITE_CONFIG=` quietly switching enforcement off is the
/// same defect as a policy file that fails to load.
///
/// Presence is [`std::fs::symlink_metadata`], matching the marker probe: a name
/// someone deliberately placed is a name claiming to be the policy, and following
/// the link would let a dangling symlink hand the decision back to the
/// environment.
///
/// Infallible, unlike `auth::path_from`, because no `ProjectDirs` lookup is
/// involved and a system path exists on every platform drep ships to.
pub fn path_from(overridden: Option<std::ffi::OsString>, machine: &Path) -> PathBuf {
    match overridden {
        Some(path) if !path.is_empty() && std::fs::symlink_metadata(machine).is_err() => {
            PathBuf::from(path)
        }
        _ => machine.to_path_buf(),
    }
}

/// What the site allows, for every repository on this machine.
///
/// `deny_unknown_fields` is the whole of the "no providers, no credentials"
/// rule: an `[[llm]]`, an `endpoint` or an `api_key` in this file is an unknown
/// key and is rejected, so there is no separate rejection list to drift from the
/// field list. It is also what makes a misspelled policy key loud rather than a
/// silent no-op.
///
/// This is the one config type that may derive `Debug`. `LlmConfig`, `AuthStore`
/// and `LlmClient` hand-write theirs because they can hold a credential; this
/// file is defined to carry none, and the attribute above is what enforces that
/// definition rather than a promise in a comment.
#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SiteConfig {
    /// Filenames whose presence in a repository refuses semantic review.
    ///
    /// Consumed by the marker refusal in `check`, which is what reads this list
    /// and stops the semantic layer before a byte of source is rendered. Parsed
    /// and validated here, and read nowhere else in this module.
    pub refuse_markers: Vec<String>,

    /// The most concurrent LLM requests any one provider may make.
    ///
    /// An `Option` rather than a `usize` sentinel so `doctor` can tell "no
    /// ceiling" from "a ceiling that happens to equal the default".
    pub max_concurrent_ceiling: Option<usize>,
}

/// The fields of this file that `drep.toml` must not be able to state.
///
/// Here rather than in `config.rs` because it is a statement about the fields
/// declared directly above it, and the decision about a new one belongs where the
/// field is added. `config::site_only_field` used to be a hard-coded
/// `tree.get("refuse_markers")` in the other module: a third field added here
/// would have compiled, said nothing, and been silently dropped from a
/// `drep.toml` that named it, because `Config` has no `deny_unknown_fields`. That
/// is the one outcome [`super::ConfigError::SiteOnlyField`] exists to prevent, and
/// it would have been reintroduced by the ordinary act of adding a policy field.
///
/// `max_concurrent_ceiling` is deliberately absent. Written in `drep.toml` it
/// changes nothing a repository could not already do by lowering its own
/// `max_concurrent`, so refusing it would be noise; the cost is that a developer
/// who writes it there is not told it does nothing.
pub const SITE_ONLY_FIELDS: &[&str] = &["refuse_markers"];

/// Fails to compile when a field is added to [`SiteConfig`] without a decision
/// about whether `drep.toml` may state it.
///
/// The exhaustive destructure is the whole point, and is the idiom `KeySource::ALL`
/// and `Severity::ALL` use for the same purpose: a list that has to be kept in
/// step with a type by hand is a list that drifts silently, and the drift here
/// ships source. Adding a field breaks this function, and the fix is one line in
/// [`SITE_ONLY_FIELDS`] or one name added below.
#[cfg(test)]
fn _every_policy_field_is_classified(site: &SiteConfig) {
    let SiteConfig {
        // Site-only: named in SITE_ONLY_FIELDS.
        refuse_markers: _,
        // Not site-only: harmless in `drep.toml`, see the note above.
        max_concurrent_ceiling: _,
    } = site;
}

/// What went wrong loading the site policy file.
///
/// A separate enum from [`super::ConfigError`], so the error's *type* names
/// which of the two files is at fault. Folding these into `ConfigError::Io` and
/// `ConfigError::Parse` would make those variants reachable from two files with
/// two different grammars, and a reader could no longer tell which grammar was
/// violated from the variant alone.
#[derive(Debug, Error)]
pub enum SiteConfigError {
    #[error(
        "could not read the site policy file {0}: {1}; `drep check` refuses to run rather than \
         report an unenforced policy as compliance"
    )]
    Read(PathBuf, std::io::Error),

    #[error(
        "could not parse the site policy file {0}: {1}; `drep check` refuses to run rather than \
         report an unenforced policy as compliance"
    )]
    Parse(PathBuf, String),

    /// The clamp runs after `super::validate`, so a ceiling of zero would slip
    /// past `ConfigError::ZeroConcurrency` and rebuild the hang it exists to
    /// prevent: a semaphore with no permits, waited on forever with no message.
    /// Rejected here so the clamp can never produce zero.
    #[error(
        "the site policy file {0} sets max_concurrent_ceiling = 0, which would leave every \
         provider unable to make a request; it must be at least 1"
    )]
    ZeroConcurrencyCeiling(PathBuf),

    /// A marker that cannot name a file matches nothing, so the policy
    /// declaring it refuses nothing while reading as though it did.
    #[error(
        "the site policy file {path} lists `{marker}` in refuse_markers, which is not a filename; \
         each marker names one file to look for, such as `.drep-no-llm`"
    )]
    UnusableRefuseMarker { path: PathBuf, marker: String },

    /// Fails closed. A policy naming markers cannot be evaluated outside a
    /// repository, and "cannot be evaluated" must not become "evaluates to
    /// allowed": that is the unenforced policy reported as compliance which
    /// every message here refuses. `cause` rather than `source` because
    /// `main.rs` prints `{err:#}`, which would otherwise print it twice.
    #[error(
        "the site policy file {path} names refuse_markers, but the repository root above {root} \
         could not be resolved: {cause}; `drep check` refuses to run rather than report an \
         unenforced policy as compliance"
    )]
    MarkerRootUnresolved {
        path: PathBuf,
        root: PathBuf,
        cause: crate::diff::GitError,
    },
}

/// A repository the site policy refuses to have reviewed by a model.
///
/// Carries both paths because the message has to answer two questions at once: a
/// developer who has never seen this needs to know which file caused it, and
/// that it came from machine policy rather than a broken install.
#[derive(Debug, Clone)]
pub struct Refusal {
    /// The marker as found, at the repository root.
    pub marker: PathBuf,
    /// The policy file that named it.
    pub policy: PathBuf,
}

/// Read the policy at `path`.
///
/// `Ok(None)` means there is no policy on this machine, which is the ordinary
/// state and the reason the return type is an `Option` rather than a
/// `SiteConfig` with empty fields: a caller cannot then confuse "no policy" with
/// "a policy that permits everything", and the two states print differently in
/// `doctor`.
pub fn load(path: &Path) -> Result<Option<SiteConfig>, SiteConfigError> {
    let content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(SiteConfigError::Read(path.to_path_buf(), err)),
    };
    let site: SiteConfig = toml::from_str(&content).map_err(|err: toml::de::Error| {
        SiteConfigError::Parse(path.to_path_buf(), err.message().to_owned())
    })?;
    validate(&site, path)?;
    Ok(Some(site))
}

/// Reject what serde cannot enforce from the type alone.
///
/// Rejects rather than repairs, following the house rule `ConfigError` states:
/// a ceiling silently bumped to 1, or a marker silently dropped, is a policy
/// doing something other than what the administrator wrote.
fn validate(site: &SiteConfig, path: &Path) -> Result<(), SiteConfigError> {
    if site.max_concurrent_ceiling == Some(0) {
        return Err(SiteConfigError::ZeroConcurrencyCeiling(path.to_path_buf()));
    }
    for marker in &site.refuse_markers {
        if !names_one_file(marker) {
            return Err(SiteConfigError::UnusableRefuseMarker {
                path: path.to_path_buf(),
                marker: marker.clone(),
            });
        }
    }
    Ok(())
}

/// Whether `candidate` is a single filename and nothing else.
///
/// Compared back against the original string so a spelling the platform
/// normalises away - `"marker/"`, which parses to one component named `marker` -
/// is rejected too. The alternative is accepting a string that names a file
/// other than the one written down.
fn names_one_file(candidate: &str) -> bool {
    let mut components = Path::new(candidate).components();
    let first = components.next();
    components.next().is_none()
        && matches!(first, Some(Component::Normal(name)) if name.to_str() == Some(candidate))
}

impl SiteConfig {
    /// `requested`, lowered to the ceiling when there is one.
    ///
    /// The single definition of the rule, so the loaded-config path and
    /// `doctor`'s raw-tree path cannot come to disagree about the same entry -
    /// the shape `auth::source_of` already exists in for the same reason.
    pub fn clamp_concurrency(&self, requested: usize) -> usize {
        match self.max_concurrent_ceiling {
            Some(ceiling) => requested.min(ceiling),
            None => requested,
        }
    }

    /// Lower every enabled provider's `max_concurrent` to the ceiling.
    ///
    /// Disabled entries are skipped, matching every other pass over the provider
    /// list: `${VAR}` expansion, field validation and `auth::resolve` all leave a
    /// parked entry alone, and clamping one would make `doctor` report a change
    /// to a provider drep never contacts.
    ///
    /// Applied to the *effective* value, whether the repository wrote
    /// `max_concurrent` or inherited the default. Skipping the defaulted ones
    /// would let a repository raise its own concurrency by deleting a line, which
    /// is the loosening this layer exists to prevent.
    ///
    /// Returns nothing: no caller wants a list of what it changed, and an unread
    /// return value is surface a later reader has to account for.
    pub fn apply(&self, config: &mut Config) {
        for llm in config.llm.iter_mut().filter(|llm| llm.enabled) {
            llm.max_concurrent = self.clamp_concurrency(llm.max_concurrent);
        }
    }

    /// The first configured marker present at the repository root above `root`.
    ///
    /// The single-directory case, for `doctor`, which reports on one directory.
    /// The gate asks [`Self::refusal_among`], because the files a check reviews
    /// are not always in the repository it was rooted in.
    pub async fn refusal_for(
        &self,
        root: &Path,
        policy: &Path,
    ) -> Result<Option<Refusal>, SiteConfigError> {
        self.refusal_among(&BTreeSet::from([root.to_path_buf()]), policy)
            .await
    }

    /// The first configured marker present at the repository root above any of
    /// `directories`.
    ///
    /// `Ok(None)` on a machine that configured none, decided before git is
    /// spawned. That short circuit is the whole reason an unaffected machine
    /// gains neither the latency nor the new failure mode: `drep check` outside a
    /// repository keeps working exactly as it does today, and only a machine that
    /// asked for the policy pays for evaluating it.
    ///
    /// The repository root, not the directory itself: a check run from a
    /// subdirectory of a marked repository is still a check on that repository's
    /// source, and consulting the given directory would let `cd src && drep
    /// check` walk straight past the policy.
    ///
    /// Plural because one run can review files from more than one repository, and
    /// then one repository's policy was consulted while another's source was
    /// sent. Each directory is resolved on its own rather than being assumed to
    /// share a root with the others: a nested checkout has its own root, and
    /// deciding otherwise from the paths alone would reimplement git's discovery
    /// rules here. The marker probe is then done once per distinct root.
    ///
    /// Presence is decided by [`std::fs::symlink_metadata`], and nothing opens
    /// the file. Not `metadata`, which follows a symlink and so answers "no" for
    /// a marker whose target is gone; not `is_file()`, which answers "no" for a
    /// directory. Both are names someone deliberately placed at the root, and
    /// either reading would let a marker silently disable the policy it was put
    /// there to invoke. Contents are never read for the same reason: a marker
    /// whose text said `allow` would be a second grammar nobody documented.
    pub async fn refusal_among(
        &self,
        directories: &BTreeSet<PathBuf>,
        policy: &Path,
    ) -> Result<Option<Refusal>, SiteConfigError> {
        if self.refuse_markers.is_empty() {
            return Ok(None);
        }

        // The roots resolve concurrently, bounded the way every other spawn fan-out
        // in this crate is bounded: `check::deterministic` runs its tool processes
        // through `buffer_unordered(TOOL_PROCESS_CONCURRENCY)` for the same reason.
        // Sequentially, this was one `git rev-parse --show-toplevel` per reviewed
        // directory at ~18ms each, added to every commit on a policy machine, and
        // the dedup below does not reduce it: `probed` dedups on the *resolved*
        // root, so it saves the marker stat, not the spawn. The refused case was
        // cheap by luck and the permitted case - which is every commit - paid all
        // of them.
        //
        // Collected, then walked in the directories' own order: a `BTreeSet` input
        // means that order is stable, and the first failure and the first marker
        // have to be the same ones a sequential walk would have found. Resolving
        // out of order and reporting in order is what keeps that.
        let mut resolved: Vec<Result<PathBuf, SiteConfigError>> =
            futures::stream::iter(directories)
                .map(|directory| async move {
                    crate::diff::repository_root(directory)
                        .await
                        .map_err(|cause| SiteConfigError::MarkerRootUnresolved {
                            path: policy.to_path_buf(),
                            root: directory.to_path_buf(),
                            cause,
                        })
                })
                .buffered(ROOT_RESOLUTION_CONCURRENCY)
                .collect()
                .await;

        let mut probed: BTreeSet<PathBuf> = BTreeSet::new();
        for outcome in resolved.drain(..) {
            let repository_root = outcome?;
            if !probed.insert(repository_root.clone()) {
                continue;
            }
            if let Some(refusal) = self.marker_at(&repository_root, policy) {
                return Ok(Some(refusal));
            }
        }
        Ok(None)
    }

    /// Whether this policy names any marker at all.
    ///
    /// So a caller can decline to build the directory set for a policy that would
    /// return immediately. The guard inside [`Self::refusal_among`] stays: this one
    /// saves the argument, not the answer.
    pub fn has_refuse_markers(&self) -> bool {
        !self.refuse_markers.is_empty()
    }

    /// The first configured marker present at one repository root.
    fn marker_at(&self, repository_root: &Path, policy: &Path) -> Option<Refusal> {
        self.refuse_markers.iter().find_map(|marker| {
            let candidate = repository_root.join(marker);
            std::fs::symlink_metadata(&candidate)
                .is_ok()
                .then(|| Refusal {
                    marker: candidate,
                    policy: policy.to_path_buf(),
                })
        })
    }
}
