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
//! - **The file is not per-user, and reads nothing from the environment.** The
//!   location is a system path rather than the `ProjectDirs` directory holding
//!   `auth.toml` and the response cache, because a policy file the policed
//!   developer can edit without privilege is not a policy file. There is no
//!   `${VAR}` expansion here for the same reason: a policy that takes its values
//!   from the environment of the process it constrains constrains nothing.
//!
//! The layering is applied by the caller, after [`super::load`] returns, which
//! is what keeps [`super::ConfigError`] a statement about `drep.toml` alone.

use std::path::{Component, Path, PathBuf};

use serde::Deserialize;
use thiserror::Error;

use super::Config;

/// The environment variable that relocates the policy file.
pub const PATH_VAR: &str = "DREP_SITE_CONFIG";

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

/// The policy path: [`PATH_VAR`] if set, else the machine-wide path.
pub fn default_path() -> PathBuf {
    path_from(std::env::var_os(PATH_VAR))
}

/// [`default_path`] with the override supplied rather than read.
///
/// Split out for the reason `auth::path_from` is: `std::env::set_var` is
/// `unsafe` in edition 2024 because another thread reading the environment is a
/// data race, and `cargo test` is multi-threaded, so the override has to be
/// suppliable to be testable at all.
///
/// An **empty** override falls back to the machine path rather than being
/// honoured as a file that does not exist. `DREP_SITE_CONFIG=` quietly switching
/// enforcement off is the same defect as a policy file that fails to load.
///
/// Infallible, unlike `auth::path_from`, because no `ProjectDirs` lookup is
/// involved and a system path exists on every platform drep ships to.
pub fn path_from(overridden: Option<std::ffi::OsString>) -> PathBuf {
    match overridden {
        Some(path) if !path.is_empty() => PathBuf::from(path),
        _ => PathBuf::from(MACHINE_PATH),
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
    /// `Ok(None)` on a machine that configured none, decided before git is
    /// spawned. That short circuit is the whole reason an unaffected machine
    /// gains neither the latency nor the new failure mode: `drep check` outside a
    /// repository keeps working exactly as it does today, and only a machine that
    /// asked for the policy pays for evaluating it.
    ///
    /// The repository root, not `root`: a check run from a subdirectory of a
    /// marked repository is still a check on that repository's source, and
    /// consulting the current directory would let `cd src && drep check` walk
    /// straight past the policy.
    ///
    /// Presence is decided by [`std::fs::symlink_metadata`], and nothing opens
    /// the file. Not `metadata`, which follows a symlink and so answers "no" for
    /// a marker whose target is gone; not `is_file()`, which answers "no" for a
    /// directory. Both are names someone deliberately placed at the root, and
    /// either reading would let a marker silently disable the policy it was put
    /// there to invoke. Contents are never read for the same reason: a marker
    /// whose text said `allow` would be a second grammar nobody documented.
    pub async fn refusal_for(
        &self,
        root: &Path,
        policy: &Path,
    ) -> Result<Option<Refusal>, SiteConfigError> {
        if self.refuse_markers.is_empty() {
            return Ok(None);
        }
        let repository_root = crate::diff::repository_root(root).await.map_err(|cause| {
            SiteConfigError::MarkerRootUnresolved {
                path: policy.to_path_buf(),
                root: root.to_path_buf(),
                cause,
            }
        })?;
        Ok(self.refuse_markers.iter().find_map(|marker| {
            let candidate = repository_root.join(marker);
            std::fs::symlink_metadata(&candidate)
                .is_ok()
                .then(|| Refusal {
                    marker: candidate,
                    policy: policy.to_path_buf(),
                })
        }))
    }
}
