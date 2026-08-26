//! TOML configuration.
//!
//! One file per repository, conventionally `drep.toml` at the root. The shape
//! is deliberate: every field has a documented default, so a partial file
//! works. Missing keys are not an error - the section just inherits the
//! default. Providers are declared as `[[llm]]`, an ordered array of tables:
//! a preference order, tried head first, each enabled entry a fallback for the
//! one before it.
//!
//! The two things this module owns that are not obvious from the field list:
//!
//! - **`${VAR}` expansion.** An api key (or any string value) can name an
//!   environment variable instead of holding the secret. The file gets
//!   committed; the secret does not. An unset variable is an error rather
//!   than an empty string, because a silent empty credential produces a
//!   confusing 401 instead of a clear "API_KEY is not set".
//! - **`max_tokens` defaults to None**, meaning no cap is sent to the model.
//!   Modern reasoning models ship 256k-1M context, and inventing a ceiling
//!   truncates them mid-thought. The option stays available for capping
//!   spend.
//!
//! ## The second layer
//!
//! This file is per-repository and `drep init` gitignores it, so a control
//! written here is per-developer and opt-in. [`site`] is the layer above it: a
//! machine-level policy file a checkout can tighten but never loosen.
//!
//! [`load`] and [`validate`] know nothing about it and take no site argument.
//! The clamp is applied by the caller, after `load` returns, which is what keeps
//! [`ConfigError`] a statement about this file alone - every one of its messages
//! numbers `[[llm]]` entries in *this* file's order, and a bare `#2` that could
//! mean either file is exactly the ambiguity those messages exist to avoid.

use std::path::{Path, PathBuf};

use open_agent::ApiProtocol;
use serde::Deserialize;
use thiserror::Error;
use toml::Value;

mod backend;
mod env;
// A submodule at its own path rather than a re-export: `site::load`,
// `site::default_path` and `site::PATH_VAR` would each collide with a name
// already here, and `config::load` against `config::site::load` is exactly the
// distinction a caller must not blur.
pub mod site;
pub use backend::{BackendKind, LlmConfig, ReasoningEffort};
// Re-exported at the parent's path rather than behind `config::env::`: `doctor`
// and `auth` both call these, and moving them was a file-size split, not a
// change of contract.
use env::{disabled_provider_indices, expand_env_except};
pub use env::{env_var_refs, env_var_refs_in, required_env_var_refs};

pub const DEFAULT_MAX_REVIEW_ROUNDS: u32 = 3;

/// The whole configuration tree, rooted at the file.
///
/// `llm` is an **array of tables** (`[[llm]]`), not a single `[llm]` section,
/// and the list is a *preference order*: [`Self::providers`] is the failover
/// chain.
///
/// `#[serde(default)]` means a file with an empty body deserializes
/// successfully; `validate` is what then rejects it, because a config
/// declaring no provider cannot run the mandatory LLM layer.
#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct Config {
    pub max_review_rounds: u32,
    pub llm: Vec<LlmConfig>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            max_review_rounds: DEFAULT_MAX_REVIEW_ROUNDS,
            llm: Vec::new(),
        }
    }
}

impl Config {
    /// The failover chain: every enabled provider, in file order.
    ///
    /// The single definition of "which providers are in play". `enabled` is
    /// an opt-*out*, so a disabled entry is skipped wherever it sits - a
    /// disabled head falls through to the entry below it rather than
    /// producing `NotConfigured`, which is what parking the local model was
    /// always meant to do.
    ///
    /// A `Vec` rather than an iterator because every caller wants a length or
    /// an index (the chain numbers its providers in error messages) and the
    /// list is at most a handful of entries.
    pub fn providers(&self) -> Vec<&LlmConfig> {
        self.llm.iter().filter(|p| p.enabled).collect()
    }
}

/// What went wrong reading or validating the configuration.
#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("could not read {0}: {1}")]
    Io(PathBuf, std::io::Error),

    #[error("could not parse {0}: {1}")]
    Parse(PathBuf, String),

    #[error("environment variable `{0}` is not set (referenced by `{1}`)")]
    EnvVarUnset(String, String),

    #[error("environment variable `{0}` is not valid UTF-8 (referenced by `{1}`)")]
    EnvVarNotUnicode(String, String),

    /// The index is the zero-based position in the file; the message renders
    /// it one-based, and says "in file order" because the *chain* numbers only
    /// the enabled entries - with a disabled head the two differ, and
    /// `[[llm]] #1` meaning different tables in the same file is worse than
    /// either convention alone.
    #[error(
        "[[llm]] #{} in file order: temperature {temperature} is outside the allowed range 0.0..=2.0",
        index + 1
    )]
    Temperature { index: usize, temperature: f32 },

    /// `max_concurrent = 0` builds a semaphore with no permits, so every
    /// request waits for one forever. Rejected at load rather than clamped: a
    /// silent bump to 1 would run a gate at a concurrency the user did not ask
    /// for, and the alternative - the documented "callers are expected to set a
    /// positive value" - is a hang with no message at all.
    #[error("[[llm]] #{} in file order: max_concurrent must be at least 1", index + 1)]
    ZeroConcurrency { index: usize },

    #[error("[[llm]] #{} in file order: timeout_secs must be at least 1", index + 1)]
    ZeroTimeout { index: usize },

    #[error("[[llm]] #{} in file order: max_tokens must be at least 1 when set", index + 1)]
    ZeroMaxTokens { index: usize },

    #[error("max_review_rounds must be at least 1")]
    ZeroReviewRounds,

    /// Rejected rather than defaulted: falling back to `openai` would post
    /// chat-completions bytes to a `/messages` endpoint, and the resulting 404
    /// reads as "the provider is down" rather than "this line has a typo".
    #[error(
        "[[llm]] #{} in file order: unknown protocol `{value}`; expected `openai` or `anthropic`",
        index + 1
    )]
    UnknownProtocol { index: usize, value: String },

    #[error(
        "[[llm]] #{} in file order: unknown backend `{value}`; expected `http` or `codex`",
        index + 1
    )]
    UnknownBackend { index: usize, value: String },

    #[error(
        "[[llm]] #{} in file order: unknown reasoning_effort `{value}`; expected `minimal`, `low`, `medium`, `high`, or `xhigh`",
        index + 1
    )]
    UnknownReasoningEffort { index: usize, value: String },

    /// Both credential fields answer the same question, so a file setting both
    /// has said two things. Rejected rather than resolved by precedence: the
    /// user who wrote the command wrote it to be run, and a silent "the literal
    /// wins" leaves them debugging a stale credential the file says nothing
    /// about.
    #[error(
        "[[llm]] #{} in file order: `api_key` and `api_key_command` are both set; remove one, \
         because a key that is already there is never re-minted by a command",
        index + 1
    )]
    AmbiguousApiKey { index: usize },

    /// An argv with no first element names no program. Rejected at load because
    /// the alternative is discovering it inside the gate, at the point where
    /// there is nothing to run and nothing useful to say about why.
    #[error(
        "[[llm]] #{} in file order: api_key_command is empty; it must name a program to run, \
         as an argv array such as [\"print-token\", \"--audience\", \"gateway\"]",
        index + 1
    )]
    EmptyApiKeyCommand { index: usize },

    #[error(
        "[[llm]] #{} in file order: backend `{backend}` does not support `{field}`",
        index + 1
    )]
    BackendField {
        index: usize,
        backend: &'static str,
        field: &'static str,
    },

    #[error(
        "[[llm]] #{} in file order: backend `{backend}` requires `{field}`",
        index + 1
    )]
    BackendMissingField {
        index: usize,
        backend: &'static str,
        field: &'static str,
    },

    #[error(
        "{0} declares no `[[llm]]` provider; drep 2.x has no deterministic-only mode. \
         Run `drep init` to write one."
    )]
    NoProviders(PathBuf),

    #[error(
        "every `[[llm]]` provider in {0} has `enabled = false`; drep 2.x has no \
         deterministic-only mode. Re-enable one, or run `drep init` to write another."
    )]
    NoEnabledProviders(PathBuf),

    /// Rejected rather than ignored, which is the one behaviour that would be
    /// worse than either: serde drops an unknown key without a word, so a
    /// developer reads `refuse_markers` in their own config, believes the
    /// repository is protected, and every review still ships its source. It is
    /// refused here rather than honoured because `drep init` gitignores this
    /// file - a copy of the control would be per-developer, and a refusal a
    /// developer can delete is not one.
    #[error(
        "{path} sets `{field}`, which is machine site policy and is read only from the site \
         policy file; this file is gitignored by `drep init`, so a copy here would be \
         per-developer and could be deleted by the developer it constrains"
    )]
    SiteOnlyField { path: PathBuf, field: &'static str },
}

/// The conventional config file location: `drep.toml` in the current directory.
///
/// Hardcoded to "drep.toml" in cwd by design: `drep init` writes this exact
/// path, so changing it here would break the contract with the init command.
pub fn default_config_path() -> PathBuf {
    PathBuf::from("drep.toml")
}

/// Parses a `protocol =` value, or `None` when it names nothing the SDK speaks.
///
/// The single definition of what a protocol name means, and it owns none of the
/// names: [`ApiProtocol::from_wire`] is the SDK's own parser, so drep cannot
/// come to disagree with the layer that acts on the answer. An absent value is
/// the default protocol rather than an error, which is what keeps every config
/// written before 0.9.0 valid.
pub fn parse_protocol(raw: Option<&str>) -> Option<ApiProtocol> {
    match raw {
        None => Some(ApiProtocol::default()),
        Some(name) => ApiProtocol::from_wire(name),
    }
}

/// Load and validate `path`.
///
/// A missing file is an error: the caller decides whether that is fatal
/// (the binary should bail) or expected (a first-run where `drep init` has
/// not been run yet). Inventing defaults for a file that does not exist
/// would silently mask a broken install.
///
/// `${VAR}` expansion happens before validation, so an unset variable is
/// reported with the variable's name rather than as a downstream parse
/// failure inside the substituted text.
pub fn load(path: &Path) -> Result<Config, ConfigError> {
    let content =
        std::fs::read_to_string(path).map_err(|err| ConfigError::Io(path.to_path_buf(), err))?;

    // `toml::from_str::<Value>` and `<Value as FromStr>::from_str` are not
    // interchangeable despite producing the same type. The former runs the
    // document parser; the latter runs `ValueDeserializer`, which
    // parses a single TOML *value* (`42`, `"text"`) and rejects a whole document
    // with "unexpected content, expected nothing".
    let mut tree: Value = toml::from_str(&content).map_err(|err: toml::de::Error| {
        ConfigError::Parse(path.to_path_buf(), err.message().to_owned())
    })?;

    // Read off the raw tree, the way `disabled_provider_indices` and
    // `backend::explicit_fields` are: `Config` has no `refuse_markers` field and
    // no `deny_unknown_fields`, so serde would deserialize this file happily and
    // say nothing.
    if let Some(field) = site_only_field(&tree) {
        return Err(ConfigError::SiteOnlyField {
            path: path.to_path_buf(),
            field,
        });
    }

    // Disabled providers are pruned from expansion, not from the tree: a
    // parked entry is inert, so an unset `${OPENROUTER_API_KEY}` in the cloud
    // block a user just switched off must not refuse to load the file. It stays
    // in `Config.llm` with its `${VAR}` unexpanded, which nothing reads - only
    // `providers()` is consulted, and it filters the entry out.
    let disabled = disabled_provider_indices(&tree);
    expand_env_except(&mut tree, path, &disabled)?;
    let explicit_fields = backend::explicit_fields(&tree);

    let config: Config = tree.try_into().map_err(|err: toml::de::Error| {
        ConfigError::Parse(path.to_path_buf(), err.message().to_owned())
    })?;

    validate(&config, path, &explicit_fields)?;
    Ok(config)
}

/// The site-policy key this file declared, if it declared one.
///
/// One name, not a list, because there is exactly one control the two files must
/// not both be able to state: `refuse_markers` reads as a security decision, and a
/// repository able to make it - or to appear to make it - is the whole thing the
/// site layer exists to take away. `max_concurrent_ceiling` is deliberately not
/// here: written in `drep.toml` it changes nothing a repository could not already
/// do by lowering its own `max_concurrent`.
fn site_only_field(tree: &Value) -> Option<&'static str> {
    tree.get("refuse_markers").map(|_| "refuse_markers")
}

/// Validate what serde cannot enforce from the type alone.
///
/// An empty provider list is rejected here rather than tolerated and caught
/// later at the LLM boundary: the LLM layer is mandatory in 2.x, so a config
/// naming no provider is a file that can never produce a passing run, and the
/// earliest place to say so is the place that read the file.
///
/// The index is carried into the temperature error because with several
/// providers "temperature 3.0 is out of range" does not say *which* one.
fn validate(
    config: &Config,
    path: &Path,
    explicit_fields: &[backend::ExplicitFields],
) -> Result<(), ConfigError> {
    if config.max_review_rounds == 0 {
        return Err(ConfigError::ZeroReviewRounds);
    }
    if config.llm.is_empty() {
        return Err(ConfigError::NoProviders(path.to_path_buf()));
    }
    // Distinct from `NoProviders` because the fix is different: one needs a
    // provider written, the other needs one re-enabled. Both are caught here
    // rather than at the LLM boundary so the message can name the file.
    if config.providers().is_empty() {
        return Err(ConfigError::NoEnabledProviders(path.to_path_buf()));
    }
    // Disabled entries are skipped. `enabled = false` means "this entry is
    // inert", and refusing to load the file because a *parked* provider names
    // an out-of-range temperature contradicts that in the one place a user
    // would notice: they parked it precisely to stop it mattering.
    for (index, llm) in config.llm.iter().enumerate().filter(|(_, l)| l.enabled) {
        backend::validate(
            llm,
            explicit_fields.get(index).copied().unwrap_or_default(),
            index,
        )?;

        if llm.max_concurrent == 0 {
            return Err(ConfigError::ZeroConcurrency { index });
        }
        if llm.timeout_secs == 0 {
            return Err(ConfigError::ZeroTimeout { index });
        }
        if llm.max_tokens == Some(0) {
            return Err(ConfigError::ZeroMaxTokens { index });
        }
        // Checked for every backend rather than only for HTTP, so the rule holds
        // above the `continue` below. `backend::validate` has already rejected
        // `api_key_command` on a Codex entry by name, so reaching here with one
        // means the backend can use it.
        if llm.api_key.is_some() && llm.api_key_command.is_some() {
            return Err(ConfigError::AmbiguousApiKey { index });
        }
        if llm.api_key_command.as_ref().is_some_and(Vec::is_empty) {
            return Err(ConfigError::EmptyApiKeyCommand { index });
        }

        if llm.backend != BackendKind::Http {
            continue;
        }
        if let Some(t) = llm.temperature
            && !(0.0..=2.0).contains(&t)
        {
            return Err(ConfigError::Temperature {
                index,
                temperature: t,
            });
        }
        // A misspelled protocol is rejected here rather than defaulted, because
        // silently falling back to `openai` would send chat-completions bytes to a
        // `/messages` endpoint and report the 404 as the endpoint being down.
        if let Some(raw) = llm.protocol.as_deref()
            && parse_protocol(Some(raw)).is_none()
        {
            return Err(ConfigError::UnknownProtocol {
                index,
                value: raw.to_owned(),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
