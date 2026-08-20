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
//! There is deliberately no `load_default`/`save_default` pair. Both were thin
//! wrappers over `default_path()`, which reads the environment - so nothing
//! could test them without `std::env::set_var`, and the mutation gate found
//! them undetectable. Every caller resolves the path once, at its own entry
//! point, and passes it down; that is also what keeps tests off the real store.
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

    /// Serializing the store failed. Distinct from [`Self::Parse`], which is
    /// about reading: a write failure reported as "could not parse" sends the
    /// reader looking at the file rather than at what drep tried to write.
    #[error("could not serialize the auth store: {0}")]
    Serialize(String),

    #[error("refusing to store an empty key for {0}")]
    EmptyKey(String),

    /// A key carrying a control character cannot be sent as an HTTP header, so
    /// storing it would defer a guaranteed failure to the first request.
    #[error("the key for {0} contains a character that cannot be sent in a header")]
    UnusableKey(String),
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
    path_from(std::env::var_os(PATH_VAR))
}

/// [`default_path`] with the override supplied rather than read.
///
/// Split out so the override can be tested without writing to the process
/// environment. `std::env::set_var` is `unsafe` in edition 2024 because another
/// thread reading the environment concurrently is a data race, and `cargo test`
/// runs tests on several threads - a "single-threaded test process" safety
/// comment would simply have been untrue.
pub fn path_from(overridden: Option<std::ffi::OsString>) -> Result<PathBuf, AuthError> {
    if let Some(path) = overridden {
        return Ok(PathBuf::from(path));
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

    /// Write the store to `path`, creating the directory if needed.
    ///
    /// The file is created mode 0600 and the directory 0700 on Unix, and the
    /// mode is applied to an *existing* file too - a store written before this
    /// ran, or one whose mode a user widened, is narrowed on the next save
    /// rather than left as found.
    pub fn save(&self, path: &Path) -> Result<(), AuthError> {
        // A bare filename has `Some("")` as its parent, which is not a
        // directory anything can create.
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            ensure_dir_private(parent)?;
        }

        let body =
            toml::to_string_pretty(self).map_err(|err| AuthError::Serialize(err.to_string()))?;

        // Created 0600 from the outset rather than written and then chmodded:
        // between those two steps the key sits in a world-readable file, which
        // is a window another process on a shared machine can read.
        write_private(path, &body)
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
        // Every use of a stored key is an HTTP header value, which cannot carry
        // a control character. Rejecting here turns a paste that picked up a
        // stray newline or an escape sequence into a message at the prompt,
        // rather than a transport failure on the first file of the first push.
        if key.chars().any(|c| c.is_control()) {
            return Err(AuthError::UnusableKey(endpoint.to_string()));
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
    let trimmed = endpoint.trim().trim_end_matches('/');

    // Only the scheme and authority are case-insensitive; a URL *path* is not.
    // Lowercasing the whole thing collapsed `/API/v1` and `/api/v1` onto one
    // entry, which for a host serving both would hand one endpoint's key to the
    // other - the same class of mistake as keying on the model alone.
    match trimmed.find("://") {
        Some(scheme_end) => format!(
            "{}://{}",
            trimmed[..scheme_end].to_ascii_lowercase(),
            lower_authority(&trimmed[scheme_end + 3..])
        ),
        // No scheme, which is what `localhost:11434/v1` looks like. It still has
        // an authority and a path, and the same rule applies to both halves -
        // lowercasing the whole string collapsed `/V1` onto `/v1` for exactly
        // the endpoints a user is most likely to type by hand.
        None => lower_authority(trimmed),
    }
}

/// Lowercase everything before the first `/` and leave the rest alone.
fn lower_authority(rest: &str) -> String {
    let host_len = rest.find('/').unwrap_or(rest.len());
    format!(
        "{}{}",
        rest[..host_len].to_ascii_lowercase(),
        &rest[host_len..]
    )
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

/// Create `dir` if it is missing, narrowing it to 0700 only when drep made it.
///
/// Only a directory drep creates is narrowed. `DREP_AUTH_PATH` can name any
/// path, so chmodding whatever happens to be its parent would let
/// `/etc/drep.toml` turn `/etc` into 0700 - breaking the system to protect one
/// file. An existing directory is the user's, and the store file's own 0600 is
/// what actually guards the key.
///
/// Shared with the model-quirks cache, which sits in the same directory: a
/// second copy of this rule that only called `create_dir_all` would leave the
/// credential store's directory world-readable whenever the cache happened to
/// be written first.
pub(crate) fn ensure_dir_private(dir: &Path) -> Result<(), AuthError> {
    let existed = dir.exists();
    std::fs::create_dir_all(dir).map_err(|err| AuthError::Write(dir.to_path_buf(), err))?;
    if !existed {
        restrict(dir, 0o700)?;
    }
    Ok(())
}

/// Write `body` to `path`, creating it readable only by its owner.
///
/// `File::create` plus a later `chmod` leaves the key in a 0644 file for the
/// duration of the write. `OpenOptions::mode` applies the mode at *creation*,
/// so there is no window. The mode is re-applied afterwards because it only
/// affects creation - an existing file keeps whatever mode it had, including
/// one a user widened.
///
/// One function with the `cfg` around the mode call, rather than two whole
/// implementations: a `#[cfg(not(unix))]` twin is not compiled here, so
/// mutating it changes nothing and the mutation gate reports an undetectable
/// survivor on every run. Windows has no mode bits, and `directories` puts the
/// file under the user's own roaming profile there.
fn write_private(path: &Path, body: &str) -> Result<(), AuthError> {
    use std::io::Write;

    // Written beside the target and renamed over it, never into it. Opening the
    // real path with `truncate` destroys the existing store before a byte of the
    // replacement is written, so a crash, a full disk or a serialization failure
    // in that window leaves the file empty or half-written - and this is the one
    // file drep holds that cannot be regenerated. `rename` is atomic within a
    // directory, so a reader sees either the whole old store or the whole new
    // one, which is why the temporary is a sibling rather than in the system
    // temp dir.
    let temporary = temp_beside(path);

    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }

    let mut file = options
        .open(&temporary)
        .map_err(|err| AuthError::Write(temporary.clone(), err))?;
    file.write_all(body.as_bytes())
        .map_err(|err| AuthError::Write(temporary.clone(), err))?;
    // Before the rename, not after: a rename that publishes a file whose
    // contents are still in the page cache can survive a crash as an empty one.
    file.sync_all()
        .map_err(|err| AuthError::Write(temporary.clone(), err))?;
    drop(file);

    // The temporary carries the mode, and `rename` keeps it - so the published
    // store is 0600 whatever the mode of the file it replaced, which is how a
    // store a user widened is narrowed again.
    restrict(&temporary, 0o600)?;

    std::fs::rename(&temporary, path).map_err(|err| {
        // Otherwise a repeatedly-failing save leaves one temporary per attempt
        // beside the store.
        let _ = std::fs::remove_file(&temporary);
        AuthError::Write(path.to_path_buf(), err)
    })
}

/// A sibling of `path` to write before renaming over it.
///
/// The whole file name is kept and a suffix appended, rather than
/// `with_extension`, which would turn `auth.toml` into `auth.tmp` and collide
/// with anything else following the same convention. `DREP_AUTH_PATH` can name
/// a file with no extension at all.
fn temp_beside(path: &Path) -> std::path::PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".drep-tmp");
    path.with_file_name(name)
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
