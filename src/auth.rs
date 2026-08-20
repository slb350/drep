//! The credential store: API keys drep holds on the user's behalf.
//!
//! `drep.toml` is a repository file. It names an endpoint, a model and a
//! protocol, all of which are shareable, and it is meant to survive being
//! committed - which is why `api_key = "${VAR}"` names an environment variable
//! rather than holding a secret. That indirection is the right answer for CI,
//! where the key arrives as a secret and nobody is at a keyboard, and the wrong
//! answer for a person setting drep up on their laptop: it makes the first-run
//! experience "now go and export something into the right shell profile".
//!
//! So keys live here instead, once per machine, outside any repository:
//!
//! ```text
//! ~/.config/drep/auth.toml     (macOS: ~/Library/Application Support/dev.slb350.drep)
//! ```
//!
//! ## Keyed by endpoint, not by provider name
//!
//! A key authenticates a *host*, so the endpoint is what it belongs to. Keying
//! by a preset name instead would mean a config that named no preset - a custom
//! endpoint, or one edited by hand after `drep init` - could not find its own
//! credential, and two presets pointed at the same host would each need their
//! own copy of one key.
//!
//! The endpoint is normalised before use (see [`normalise`]) so a trailing
//! slash or a difference in case does not hide a key from the config that
//! stored it.
//!
//! ## Resolution order
//!
//! [`resolve`] fills in what `drep.toml` left unset, and an explicit value in
//! the file always wins. A user who writes `api_key = "${OPENROUTER_API_KEY}"`
//! has said where the key comes from, and silently preferring a stored one
//! would make the file lie about what the run used.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::config::Config;

/// Where a provider's key came from, for [`doctor`](crate::cli::doctor) to report.
///
/// The point of naming the source is that "it works on my machine" and "it works
/// in CI" are different configurations, and the difference is invisible in
/// `drep.toml` once a stored key exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeySource {
    /// The config named an environment variable or a literal, and it resolved.
    Config,
    /// The config named nothing and the store had one for this endpoint.
    Store,
    /// Neither. The provider will authenticate as `not-needed`, which a local
    /// server accepts and a cloud one answers with a 401.
    Missing,
}

impl KeySource {
    /// What `doctor` prints for this source.
    ///
    /// The single definition of the wording. `doctor` hand-wrote its own copy
    /// of these three strings while this method sat unused, which is two places
    /// to keep in step for no gain.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Config => "from drep.toml",
            Self::Store => "from the drep auth store",
            Self::Missing => "not set - run `drep auth login` or add `api_key` to drep.toml",
        }
    }
}

/// Keys held for this machine, keyed by normalised endpoint.
///
/// `Serialize`/`Deserialize` drive the on-disk TOML directly; there is no
/// separate wire type because the file is drep's own and has one shape.
#[derive(Default, Serialize, Deserialize)]
pub struct AuthStore {
    /// Endpoint to key. A `BTreeMap` so the file is written in a stable order
    /// and a re-save produces no spurious diff.
    #[serde(default)]
    keys: BTreeMap<String, String>,
}

/// Hand-written so a key cannot reach a log.
///
/// The same reasoning as `LlmConfig` and `LlmClient`: a derived `Debug` prints
/// every value, so one `{:?}` anywhere would emit every credential the user has.
/// The endpoints are printed because they are not secret and they are the useful
/// half when debugging.
impl std::fmt::Debug for AuthStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthStore")
            .field("endpoints", &self.keys.keys().collect::<Vec<_>>())
            .finish()
    }
}

/// What can go wrong reading or writing the store.
#[derive(Debug, Error)]
pub enum AuthError {
    #[error("no config directory for this platform; set the key in drep.toml instead")]
    NoConfigDir,

    #[error("could not read {0}: {1}")]
    Read(PathBuf, std::io::Error),

    #[error("could not write {0}: {1}")]
    Write(PathBuf, std::io::Error),

    #[error("could not parse {0}: {1}")]
    Parse(PathBuf, String),

    #[error("refusing to store an empty key for {0}")]
    EmptyKey(String),
}

/// The environment variable that relocates the store.
pub const PATH_VAR: &str = "DREP_AUTH_PATH";

/// The store location: [`PATH_VAR`] if set, else the platform's config dir.
///
/// The platform path comes from `directories::ProjectDirs` with the same triple
/// `Cache::default_root` uses, so drep's two user-level directories are siblings
/// under one application identity rather than two unrelated paths.
///
/// The override exists because there is otherwise **no way to run drep against a
/// scratch store**. `directories` follows each platform's own convention rather
/// than the XDG variables, so on macOS `XDG_CONFIG_HOME` is ignored entirely and
/// a command run to try something out writes into the real store - which is how
/// a test key ended up in one. It also serves the ordinary case of keeping
/// credentials somewhere deliberate, such as a mounted volume.
pub fn default_path() -> Result<PathBuf, AuthError> {
    if let Some(overridden) = std::env::var_os(PATH_VAR) {
        return Ok(PathBuf::from(overridden));
    }
    directories::ProjectDirs::from("dev", "slb350", "drep")
        .map(|dirs| dirs.config_dir().join("auth.toml"))
        .ok_or(AuthError::NoConfigDir)
}

impl AuthStore {
    /// An empty store, for a machine that has never stored a key.
    pub fn new() -> Self {
        Self::default()
    }

    /// Read the store at `path`.
    ///
    /// A **missing file is an empty store**, not an error: never having stored a
    /// key is the normal first-run state, and making the caller distinguish it
    /// from a real read failure would put that branch at every call site. A file
    /// that exists but cannot be read or parsed *is* an error, because silently
    /// treating a corrupt store as empty would send a user to re-paste keys they
    /// already have.
    pub fn load(path: &Path) -> Result<Self, AuthError> {
        let content = match std::fs::read_to_string(path) {
            Ok(content) => content,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Self::new()),
            Err(err) => return Err(AuthError::Read(path.to_path_buf(), err)),
        };
        toml::from_str(&content)
            .map_err(|err: toml::de::Error| AuthError::Parse(path.to_path_buf(), err.to_string()))
    }

    /// Read the store from [`default_path`].
    pub fn load_default() -> Result<Self, AuthError> {
        Self::load(&default_path()?)
    }

    /// Write the store to `path`, creating the directory if needed.
    ///
    /// The file is created mode 0600 and the directory 0700 on Unix, and the
    /// mode is applied to an *existing* file too - a store written before this
    /// ran, or one whose mode a user widened, is narrowed on the next save
    /// rather than left as found.
    pub fn save(&self, path: &Path) -> Result<(), AuthError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|err| AuthError::Write(parent.to_path_buf(), err))?;
            restrict(parent, 0o700)?;
        }

        let body = toml::to_string_pretty(self)
            .map_err(|err| AuthError::Parse(path.to_path_buf(), err.to_string()))?;
        std::fs::write(path, body).map_err(|err| AuthError::Write(path.to_path_buf(), err))?;
        restrict(path, 0o600)
    }

    /// Write the store to [`default_path`].
    pub fn save_default(&self) -> Result<(), AuthError> {
        self.save(&default_path()?)
    }

    /// The key held for `endpoint`, if any.
    pub fn get(&self, endpoint: &str) -> Option<&str> {
        self.keys.get(&normalise(endpoint)).map(String::as_str)
    }

    /// Store `key` for `endpoint`, replacing any previous one.
    ///
    /// An empty key is rejected rather than stored: it would satisfy every
    /// "is a key present" check and then fail at the endpoint with a 401, which
    /// is the confusing-empty-credential failure `${VAR}` expansion already
    /// refuses for the same reason. Surrounding whitespace is trimmed, because a
    /// pasted key routinely carries a trailing newline.
    pub fn set(&mut self, endpoint: &str, key: &str) -> Result<(), AuthError> {
        let key = key.trim();
        if key.is_empty() {
            return Err(AuthError::EmptyKey(endpoint.to_string()));
        }
        self.keys.insert(normalise(endpoint), key.to_string());
        Ok(())
    }

    /// Forget the key for `endpoint`. Returns whether one was held.
    pub fn remove(&mut self, endpoint: &str) -> bool {
        self.keys.remove(&normalise(endpoint)).is_some()
    }

    /// Every endpoint with a stored key, in sorted order. Never the keys.
    pub fn endpoints(&self) -> Vec<&str> {
        self.keys.keys().map(String::as_str).collect()
    }

    /// Whether the store holds nothing.
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }
}

/// Canonical form of an endpoint for use as a store key.
///
/// Lowercased and stripped of trailing slashes. `https://API.Z.AI/v1/` and
/// `https://api.z.ai/v1` are the same host and the same credential, and a store
/// that disagreed would report "no key" for a config that had just stored one -
/// a failure with no visible cause, since both spellings look right.
///
/// Deliberately no more than that: the path is significant (`/v1` and
/// `/anthropic/v1` are different APIs on the same host, and can carry different
/// keys), so nothing beyond the trailing slash is trimmed.
pub fn normalise(endpoint: &str) -> String {
    endpoint.trim().trim_end_matches('/').to_ascii_lowercase()
}

/// Fill in keys the config left unset, and report where each one came from.
///
/// Returns one [`KeySource`] per entry in `config.llm`, positionally - including
/// the disabled ones, so a caller numbering providers by file position and a
/// caller numbering by chain position both index correctly.
///
/// **Disabled entries are skipped**, matching every other pass over the provider
/// list: `${VAR}` expansion and field validation already leave a parked entry
/// alone, and looking a key up for one would report a missing credential for a
/// provider that is never contacted.
pub fn resolve(config: &mut Config, store: &AuthStore) -> Vec<KeySource> {
    config
        .llm
        .iter_mut()
        .map(|llm| {
            let source = source_of(
                llm.api_key.as_deref(),
                llm.endpoint.as_deref(),
                llm.enabled,
                store,
            );
            if source == KeySource::Store
                && let Some(endpoint) = llm.endpoint.as_deref()
                && let Some(key) = store.get(endpoint)
            {
                llm.api_key = Some(key.to_string());
            }
            source
        })
        .collect()
}

/// Where a provider's key will come from, given what its config names.
///
/// The precedence rule itself, in one place. [`resolve`] applies it to a loaded
/// `Config`; `doctor` applies it to the *raw* TOML tree, which it has to read
/// separately so a `${VAR}` prints as itself rather than being swallowed by the
/// variable-not-set error. Two readers of one rule is the shape
/// `config::env_var_refs_in` already exists to prevent: doctor once carried a
/// narrower copy of that scanner and reported a config as fine that `check`
/// refused to load.
pub fn source_of(
    api_key: Option<&str>,
    endpoint: Option<&str>,
    enabled: bool,
    store: &AuthStore,
) -> KeySource {
    if !enabled {
        return KeySource::Missing;
    }
    if api_key.is_some() {
        return KeySource::Config;
    }
    match endpoint {
        Some(endpoint) if store.get(endpoint).is_some() => KeySource::Store,
        _ => KeySource::Missing,
    }
}

/// Narrow `path` to `mode` on Unix. A no-op elsewhere.
///
/// Windows has no mode bits and `directories` puts the file under the user's
/// roaming profile, which is already user-scoped; failing the save there would
/// refuse to store a key for no gain.
#[cfg(unix)]
fn restrict(path: &Path, mode: u32) -> Result<(), AuthError> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .map_err(|err| AuthError::Write(path.to_path_buf(), err))
}

#[cfg(not(unix))]
fn restrict(_path: &Path, _mode: u32) -> Result<(), AuthError> {
    Ok(())
}

#[cfg(test)]
mod tests;
