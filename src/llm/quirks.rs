//! What one *model* accepts, as opposed to what its provider usually does.
//!
//! `temperature` and `max_tokens` are properties of a model, and `drep init`
//! used to guess them per *preset*: `kimi` sent no temperature because `k3`
//! refuses one, and a required `max_tokens` of 200,000 because that is a number
//! the endpoint accepts. Both guesses hold for the preset's default model and
//! are unverified for every other model that endpoint serves - and the wizard
//! exists so the user can pick one of those.
//!
//! A wrong guess is not cosmetic. A `temperature` a model rejects is a 400, and
//! a 400 neither fails over nor retries, so the provider is configured and can
//! never answer. The chosen model is therefore what decides.
//!
//! ## Why a registry, when [`crate::llm::models`] says not to
//!
//! That module rejected a vendored catalogue for the question *it* answers -
//! "which models does this account's plan serve" - and the rejection stands:
//! only the endpoint knows that, and a third-party index would go stale exactly
//! as the hardcoded defaults did.
//!
//! This is a different question. `GET {base_url}/models` returns ids and
//! nothing else: not one of the three subscription endpoints says whether a
//! model accepts `temperature` or what its output ceiling is. The endpoint
//! cannot answer it, so the only sources are a hand-maintained table inside
//! drep - which is the staleness the listing removed - or an index that already
//! tracks it. models.dev publishes both fields per model.
//!
//! ## Only ever narrowing
//!
//! The registry may **withdraw** `temperature` and may **replace** a required
//! `max_tokens` with the model's own limit. It may never introduce either.
//! Sending a parameter drep would otherwise have omitted is the direction that
//! produces a 400; omitting one it would have sent costs nothing but default
//! sampling. An index that disagrees with an endpoint therefore cannot break a
//! provider that worked before it existed.
//!
//! ## Failure is never fatal
//!
//! Same contract as [`crate::llm::models`]: a missing cache, an unreadable one,
//! a document that will not parse, an unreachable models.dev, a model released
//! this morning - every one of them falls back to the preset's own values,
//! which is what `drep init` wrote before this module existed. Nothing here can
//! stop `drep init`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Where the raw document lives.
pub const REGISTRY_URL: &str = "https://models.dev/api.json";

/// How long to wait for it.
///
/// Twice [`crate::llm::models`]'s listing timeout, because the document is ~4 MB
/// rather than a page of ids and 10 s would fail on an ordinary link. Still
/// bounded: this runs once between two prompts, at most once a week, and every
/// way it can fail lands on the preset's values.
const TIMEOUT: Duration = Duration::from_secs(20);

/// How old a cached registry may be before it is refetched.
///
/// A week. What a model accepts does not change once it has shipped, so the
/// refresh is about models that did not exist when the cache was written.
const MAX_AGE: u64 = 7 * 24 * 60 * 60;

/// The cache file's name, under the directory [`path_from`] resolves.
const FILE_NAME: &str = "model-quirks.toml";

/// The environment variable that relocates the cache.
pub const PATH_VAR: &str = "DREP_QUIRKS_PATH";

/// What `drep init` should write for one model.
///
/// Built from the preset, then narrowed by the registry when it knows the
/// model. `max_tokens_from_registry` exists so the rendered comment can say
/// something true: "this is the model's own limit" is a claim, it lands in a
/// file the user commits, and it is false whenever the value came from the
/// preset's fallback instead.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quirks {
    /// Sampling temperature, or `None` to send none at all.
    pub temperature: Option<f32>,
    /// Completion ceiling, or `None` to send none.
    pub max_tokens: Option<u32>,
    /// Whether `max_tokens` is the model's own published limit.
    pub max_tokens_from_registry: bool,
}

/// The two facts drep reads about a model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelFacts {
    /// Whether the model accepts a `temperature` parameter at all.
    ///
    /// Defaulted to `true` when the document omits it - 448 of models.dev's
    /// entries do - because withdrawing the parameter is a decision, and
    /// silence is not evidence for it.
    #[serde(default = "yes")]
    pub temperature: bool,
    /// The model's own completion ceiling, when it publishes one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_limit: Option<u32>,
}

/// The `#[serde(default)]` for [`ModelFacts::temperature`].
fn yes() -> bool {
    true
}

/// models.dev, distilled to what drep reads.
///
/// Keyed by endpoint rather than by the vendor's provider id, because the
/// endpoint is what `drep.toml` carries and what the user typed. A `custom`
/// entry pointed at a host drep ships no preset for still joins; a provider
/// models.dev publishes with no `api` URL simply never does, and its models
/// keep the preset's values.
///
/// Keying on the model id alone was never an option: one open model is served
/// by a dozen hosts under the same name, which is the identity mistake
/// `Provider::cache_key` already exists to avoid.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Registry {
    /// When it was distilled, in seconds since the Unix epoch.
    ///
    /// First field deliberately: TOML requires every scalar in a table to
    /// precede the sub-tables, and `providers` is nothing but sub-tables.
    fetched_at: u64,
    /// Normalised endpoint -> model id -> facts.
    #[serde(default)]
    providers: BTreeMap<String, BTreeMap<String, ModelFacts>>,
}

/// Why a registry could not be produced. Every variant is non-fatal.
#[derive(Debug, Error)]
pub enum QuirksError {
    #[error("could not reach the model registry: {0}")]
    Transport(String),

    #[error("the model registry could not be read: {0}")]
    Malformed(String),

    #[error("could not write the model registry cache to {0}: {1}")]
    Cache(PathBuf, String),
}

/// Where the wizard gets the registry.
///
/// A trait for the same reason [`crate::llm::models::ModelSource`] is one: the
/// wizard's tests inject a stub, so no test in this crate reaches models.dev.
pub trait QuirksSource {
    /// The registry, or why there isn't one.
    #[allow(async_fn_in_trait)]
    async fn registry(&self) -> Result<Registry, QuirksError>;
}

/// Where the raw document comes from, underneath the cache.
///
/// Separate from [`QuirksSource`] so [`Cached`]'s own behaviour - freshness,
/// writing, and what happens when the network is down but a copy is on disk -
/// is testable with a stub in place of the network.
pub trait Fetch {
    /// The raw models.dev document.
    #[allow(async_fn_in_trait)]
    async fn document(&self) -> Result<String, QuirksError>;
}

/// A borrowed fetcher fetches.
///
/// So a caller can keep the fetcher and inspect it afterwards - which is how a
/// test tells "the cache answered" from "the cache was ignored and the answer
/// happened to match" - rather than handing ownership to [`Cached`].
impl<F: Fetch> Fetch for &F {
    async fn document(&self) -> Result<String, QuirksError> {
        (*self).document().await
    }
}

/// The real thing: one HTTP GET.
#[derive(Debug, Clone)]
pub struct Http {
    url: String,
    /// The largest body this fetcher will read.
    ///
    /// A field rather than a bare constant so the boundary is testable: the
    /// production value is 32 MB, and a test that had to build a body that size
    /// to check the comparison would be the reason nobody wrote one.
    max_bytes: u64,
}

impl Http {
    /// A fetcher for `url` - [`REGISTRY_URL`] in production.
    ///
    /// The URL is a parameter rather than baked into the request for the same
    /// reason [`crate::llm::models::Http`] takes an endpoint: a status check
    /// against a real server is the only thing that can tell a success from a
    /// failure, and one nothing exercises is a mutation survivor. It also lets
    /// a caller point drep at a mirror of the document.
    pub fn new(url: &str) -> Self {
        Self {
            url: url.to_string(),
            max_bytes: MAX_DOCUMENT_BYTES,
        }
    }

    /// The same fetcher with a different size ceiling.
    ///
    /// Exists for the tests that pin the boundary. Production always uses
    /// [`MAX_DOCUMENT_BYTES`], which `new` applies.
    pub fn with_max_bytes(mut self, max_bytes: u64) -> Self {
        self.max_bytes = max_bytes;
        self
    }
}

impl Fetch for Http {
    async fn document(&self) -> Result<String, QuirksError> {
        let client = reqwest::Client::builder()
            .timeout(TIMEOUT)
            .build()
            .map_err(|err| QuirksError::Transport(err.to_string()))?;

        let response = client
            .get(&self.url)
            .send()
            .await
            .map_err(|err| QuirksError::Transport(err.to_string()))?;

        if !response.status().is_success() {
            return Err(QuirksError::Transport(format!(
                "HTTP {}",
                response.status().as_u16()
            )));
        }

        // A `Content-Length` well past anything the real document could be is
        // refused before a byte of it is read. The timeout is not a size bound:
        // a fast host can stream far more than this within it, and the body is
        // buffered whole.
        if let Some(len) = response.content_length()
            && len > self.max_bytes
        {
            return Err(QuirksError::Transport(format!(
                "the registry document is {len} bytes, past the {}-byte limit",
                self.max_bytes
            )));
        }

        // Read in chunks rather than `text()`, so a response that declares no
        // length - chunked transfer encoding does not - is still bounded.
        let mut body = Vec::new();
        let mut stream = response;
        while let Some(chunk) = stream
            .chunk()
            .await
            .map_err(|err| QuirksError::Transport(err.to_string()))?
        {
            body.extend_from_slice(&chunk);
            if body.len() as u64 > self.max_bytes {
                return Err(QuirksError::Transport(format!(
                    "the registry document exceeded the {}-byte limit",
                    self.max_bytes
                )));
            }
        }
        String::from_utf8(body)
            .map_err(|err| QuirksError::Malformed(crate::text::excerpt(&err.to_string(), 120)))
    }
}

/// A registry read from disk, refetched when it has gone stale.
///
/// The cache path is a constructor argument rather than something read from the
/// environment inside, for the reason `auth` has no `load_default`: a function
/// that resolves its own path cannot be tested without writing to the process
/// environment, and the mutation gate reports it as an undetectable survivor.
pub struct Cached<F = Http> {
    path: Option<PathBuf>,
    fetcher: F,
    now: u64,
}

impl Cached<Http> {
    /// Cache at `path`, fetching from models.dev when it is missing or old.
    ///
    /// `None` means no cache is available at all (no platform config
    /// directory), which costs a fetch per run rather than an error.
    pub fn new(path: Option<PathBuf>) -> Self {
        Self {
            path,
            fetcher: Http::new(REGISTRY_URL),
            now: unix_now(),
        }
    }
}

#[cfg(test)]
impl<F: Fetch> Cached<F> {
    /// [`Cached::new`] with the clock and the fetcher supplied.
    pub(crate) fn at(path: Option<PathBuf>, fetcher: F, now: u64) -> Self {
        Self { path, fetcher, now }
    }
}

impl<F: Fetch> QuirksSource for Cached<F> {
    async fn registry(&self) -> Result<Registry, QuirksError> {
        let cached = self.path.as_deref().and_then(Registry::load);
        if let Some(registry) = &cached
            && !registry.is_stale(self.now)
        {
            return Ok(registry.clone());
        }

        let fetched = self
            .fetcher
            .document()
            .await
            .and_then(|body| Registry::distil(&body, self.now));

        match fetched {
            Ok(registry) => {
                if let Some(path) = &self.path {
                    // A cache drep cannot write is a slower run, not a failure:
                    // the registry in hand is the same either way.
                    let _ = registry.save(path);
                }
                Ok(registry)
            }
            // A stale copy still describes models that already existed, which is
            // every model but this week's. Refusing to use it would make a user
            // who has been offline for eight days strictly worse off than one
            // offline for six.
            Err(err) => cached.ok_or(err),
        }
    }
}

impl Registry {
    /// What the registry knows about `model` at `endpoint`, if anything.
    pub fn facts(&self, endpoint: &str, model: &str) -> Option<&ModelFacts> {
        self.providers
            .get(&crate::auth::normalise(endpoint))?
            .get(model)
    }

    /// Whether this copy is older than [`MAX_AGE`] at `now`.
    ///
    /// A `fetched_at` in the future - a clock that moved backwards - saturates
    /// to an age of zero and reads as fresh, rather than wrapping into a
    /// permanent refetch.
    pub fn is_stale(&self, now: u64) -> bool {
        now.saturating_sub(self.fetched_at) > MAX_AGE
    }

    /// Read the cache at `path`, or `None` for any reason it cannot be used.
    ///
    /// Missing, unreadable and unparseable collapse deliberately: all three mean
    /// "refetch", none of them is something a user can act on, and reporting a
    /// corrupt cache would be an error message about a file drep is about to
    /// overwrite anyway. That is the opposite of `AuthStore::load`, which errors
    /// on a corrupt store - because there the file holds something irreplaceable.
    pub fn load(path: &Path) -> Option<Self> {
        toml::from_str(&std::fs::read_to_string(path).ok()?).ok()
    }

    /// Write the cache to `path`, creating the directory if needed.
    pub fn save(&self, path: &Path) -> Result<(), QuirksError> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            // Through `auth`'s helper, not `create_dir_all`: this file shares a
            // directory with `auth.toml`, so creating it here without narrowing
            // it to 0700 would leave the credential store's own directory
            // world-readable whenever `drep init` happened to cache first.
            crate::auth::ensure_dir_private(parent)
                .map_err(|err| QuirksError::Cache(parent.to_path_buf(), err.to_string()))?;
        }
        let body = toml::to_string(self)
            .map_err(|err| QuirksError::Cache(path.to_path_buf(), err.to_string()))?;

        // Written beside the target and renamed over it. `fs::write` truncates
        // in place, so a crash mid-write - or two `drep init` runs sharing a
        // cache path - leaves a half-written file. That is self-healing, since
        // an unparseable cache falls back to the presets and is refetched, but
        // it costs a 4 MB download to recover from something a rename avoids.
        // `rename` is atomic within a directory, which is why the temporary
        // sits next to the target rather than in the system temp dir.
        let temporary = path.with_extension("toml.tmp");
        std::fs::write(&temporary, body)
            .map_err(|err| QuirksError::Cache(temporary.clone(), err.to_string()))?;
        std::fs::rename(&temporary, path).map_err(|err| {
            // A failed rename leaves the temporary behind; clearing it keeps a
            // failure from accumulating one file per attempt.
            let _ = std::fs::remove_file(&temporary);
            QuirksError::Cache(path.to_path_buf(), err.to_string())
        })
    }

    /// Distil models.dev's document down to what drep reads.
    ///
    /// The source is ~4 MB across ~190 providers and ~6,800 models; a boolean
    /// and an integer per model is ~600 KB, measured against the real document.
    /// A provider with no
    /// `api` URL is dropped rather than kept under its vendor id: with no
    /// endpoint to join on, an entry could only ever be matched by model name -
    /// which is how one open model served by two hosts gets the other's facts.
    pub fn distil(body: &str, fetched_at: u64) -> Result<Self, QuirksError> {
        let raw: BTreeMap<String, RawProvider> = serde_json::from_str(body)
            .map_err(|err| QuirksError::Malformed(crate::text::excerpt(&err.to_string(), 120)))?;

        let mut providers: BTreeMap<String, BTreeMap<String, ModelFacts>> = BTreeMap::new();
        for provider in raw.into_values() {
            let Some(api) = provider.api.filter(|api| !api.trim().is_empty()) else {
                continue;
            };
            let models: BTreeMap<String, ModelFacts> = provider
                .models
                .into_iter()
                .map(|(id, model)| {
                    (
                        id,
                        ModelFacts {
                            temperature: model.temperature,
                            output_limit: model.limit.and_then(|limit| limit.output),
                        },
                    )
                })
                .collect();
            // Merged, not inserted. Two providers can publish the same `api`
            // URL - `minimax` and `minimax-coding-plan` both publish
            // `https://api.minimax.io/anthropic/v1`, which is drep's own
            // MINIMAX preset - and `insert` would silently discard whichever
            // arrived first, taking its models with it.
            providers
                .entry(crate::auth::normalise(&api))
                .or_default()
                .extend(models);
        }

        if providers.is_empty() {
            return Err(QuirksError::Malformed(
                "the document named no provider with an endpoint".to_string(),
            ));
        }

        Ok(Self {
            fetched_at,
            providers,
        })
    }
}

/// What `defaults` becomes once the registry has been consulted.
///
/// Narrowing only, in both fields. `temperature` is withdrawn when the registry
/// says the model refuses it and otherwise left exactly as the preset set it; a
/// required `max_tokens` takes the model's own ceiling and an absent one stays
/// absent. Whether the field is *required* remains a property of the endpoint,
/// which is why `defaults.max_tokens.is_some()` still decides that `k3` gets a
/// value and `glm-5.3` does not.
pub fn resolve(
    registry: Option<&Registry>,
    defaults: Quirks,
    endpoint: &str,
    model: &str,
) -> Quirks {
    let Some(facts) = registry.and_then(|registry| registry.facts(endpoint, model)) else {
        return defaults;
    };

    Quirks {
        temperature: if facts.temperature {
            defaults.temperature
        } else {
            None
        },
        // `min`, not replace. The preset's value is one drep has verified the
        // endpoint accepts; a published limit *above* it is a claim drep has
        // not tested, and raising a required ceiling is the direction that
        // yields a 400 - which by invariant neither fails over nor retries.
        // Lowering only ever costs a shorter answer.
        max_tokens: defaults.max_tokens.map(|fallback| {
            facts
                .output_limit
                .map_or(fallback, |limit| limit.min(fallback))
        }),
        max_tokens_from_registry: defaults
            .max_tokens
            .is_some_and(|fallback| facts.output_limit.is_some_and(|limit| limit < fallback)),
    }
}

/// The largest registry document drep will read into memory.
///
/// The live document is about 4 MB. 32 MB is a wide margin for growth and still
/// refuses a mirror, a redirect to something else, or a compromised host trying
/// to make `drep init` allocate without bound - which the timeout alone does
/// not prevent, since a fast host can send a great deal inside it.
const MAX_DOCUMENT_BYTES: u64 = 32 * 1024 * 1024;

/// The cache location: [`PATH_VAR`] if set, else beside `auth.toml`.
pub fn default_path() -> Option<PathBuf> {
    path_from(std::env::var_os(PATH_VAR))
}

/// [`default_path`] with the override supplied rather than read.
///
/// Split for the reason `auth::path_from` is: `std::env::set_var` is `unsafe`
/// in edition 2024 and `cargo test` is multi-threaded, so an override read
/// inside the function could not be tested at all.
///
/// The directory is `config_dir()` under the same `ProjectDirs` triple as the
/// credential store, so drep's user-level files stay siblings under one
/// application identity rather than scattering across two conventions. `None` -
/// a platform with no config directory - means the run fetches and does not
/// cache, which is slower and never wrong.
pub fn path_from(overridden: Option<std::ffi::OsString>) -> Option<PathBuf> {
    if let Some(path) = overridden {
        return Some(PathBuf::from(path));
    }
    directories::ProjectDirs::from("dev", "slb350", "drep")
        .map(|dirs| dirs.config_dir().join(FILE_NAME))
}

/// Seconds since the Unix epoch, or 0 for a clock set before it.
fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or(0)
}

/// One provider in models.dev's document. Every other field serde ignores.
#[derive(Debug, Deserialize)]
struct RawProvider {
    #[serde(default)]
    api: Option<String>,
    #[serde(default)]
    models: BTreeMap<String, RawModel>,
}

/// One model. Only the two fields drep reads are named.
#[derive(Debug, Deserialize)]
struct RawModel {
    #[serde(default = "yes")]
    temperature: bool,
    #[serde(default)]
    limit: Option<RawLimit>,
}

/// A model's context and completion ceilings; drep reads only the second.
#[derive(Debug, Deserialize)]
struct RawLimit {
    #[serde(default)]
    output: Option<u32>,
}

#[cfg(test)]
mod tests;
