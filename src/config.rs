//! TOML configuration.
//!
//! One file per repository, conventionally `drep.toml` at the root. The shape
//! is deliberate: every field has a documented default, so a partial file
//! works. Missing keys are not an error - the section just inherits the
//! default. Providers are declared as `[[llm]]`, an ordered array of tables,
//! even while only the first entry is consulted.
//!
//! The two things this module owns that are not obvious from the field list:
//!
//! - **`${VAR}` expansion.** An api key (or any string value) can name an
//!   environment variable instead of holding the secret. The file gets
//!   committed; the secret does not. An unset variable is an error rather
//!   than an empty string, because a silent empty credential produces a
//!   confusing 401 instead of a clear "GITHUB_TOKEN is not set".
//! - **`max_tokens` defaults to None**, meaning no cap is sent to the model.
//!   Modern reasoning models ship 256k-1M context, and inventing a ceiling
//!   truncates them mid-thought. The option stays available for capping
//!   spend.

use std::env;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use thiserror::Error;
use toml::Value;

/// The whole configuration tree, rooted at the file.
///
/// `llm` is an **array of tables** (`[[llm]]`), not a single `[llm]` section,
/// and it is that shape from the day `drep init` first wrote one. Multi-provider
/// failover is a later phase, but the file format cannot change underneath a
/// file drep itself wrote - so the list arrives first with exactly one entry in
/// it, and failover later fills the tail rather than rewriting the head.
///
/// `#[serde(default)]` means a file with an empty body deserializes
/// successfully; [`validate`] is what then rejects it, because a config
/// declaring no provider cannot run the mandatory LLM layer.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    pub llm: Vec<LlmConfig>,
}

impl Config {
    /// The provider requests go to first.
    ///
    /// `Option` rather than an indexing method that cannot fail: [`load`]
    /// rejects an empty list, but `Config` is also constructible directly (in
    /// tests, and by any later caller), and a panic inside the commit gate is
    /// a worse failure than an error message.
    pub fn primary(&self) -> Option<&LlmConfig> {
        self.llm.first()
    }
}

/// LLM section.
///
/// Field ordering matches the spec; every field has its documented default
/// here so partial files work. `enabled` is the one a partial file would
/// commonly set to `true` without configuring anything else, in which case
/// `new()` rejects the resulting `LlmClient` with `NotConfigured` rather
/// than guessing an endpoint.
#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct LlmConfig {
    pub enabled: bool,
    pub endpoint: Option<String>,
    pub model: Option<String>,
    pub api_key: Option<String>,
    pub temperature: f32,
    pub max_tokens: Option<u32>,
    pub timeout_secs: u64,
    pub max_retries: u32,
    pub max_concurrent: usize,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            endpoint: None,
            model: None,
            api_key: None,
            temperature: 0.2,
            max_tokens: None,
            timeout_secs: 60,
            max_retries: 3,
            max_concurrent: 3,
        }
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

    #[error("[[llm]] #{0}: temperature {1} is outside the allowed range 0.0..=2.0")]
    Temperature(usize, f32),

    #[error(
        "{0} declares no `[[llm]]` provider; drep 2.x has no deterministic-only mode. \
         Run `drep init` to write one."
    )]
    NoProviders(PathBuf),
}

/// The conventional config file location: `drep.toml` in the current directory.
///
/// Hardcoded to "drep.toml" in cwd by design: `drep init` writes this exact
/// path, so changing it here would break the contract with the init command.
pub fn default_config_path() -> PathBuf {
    PathBuf::from("drep.toml")
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

    // `toml::from_str::<Value>` and `<Value as FromStr>::from_str` are NOT
    // interchangeable in toml 1.x, despite producing the same type. The former
    // runs the document parser; the latter runs `ValueDeserializer`, which
    // parses a single TOML *value* (`42`, `"text"`) and rejects a whole document
    // with "unexpected content, expected nothing".
    let mut tree: Value = toml::from_str(&content).map_err(|err: toml::de::Error| {
        ConfigError::Parse(path.to_path_buf(), err.message().to_owned())
    })?;

    expand_env_in(&mut tree, path)?;

    let config: Config = tree.try_into().map_err(|err: toml::de::Error| {
        ConfigError::Parse(path.to_path_buf(), err.message().to_owned())
    })?;

    validate(&config, path)?;
    Ok(config)
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
fn validate(config: &Config, path: &Path) -> Result<(), ConfigError> {
    if config.llm.is_empty() {
        return Err(ConfigError::NoProviders(path.to_path_buf()));
    }
    for (index, llm) in config.llm.iter().enumerate() {
        let t = llm.temperature;
        if !(0.0..=2.0).contains(&t) {
            return Err(ConfigError::Temperature(index, t));
        }
    }
    Ok(())
}

/// Walk every string in the parsed TOML tree and expand `${VAR}` references.
///
/// Applied to the whole tree rather than per-field so a future field added
/// to `LlmConfig` inherits the behaviour without remembering to opt in. The
/// reference is the path that contained it, so an unset variable's error
/// message points at the file rather than the variable alone.
fn expand_env_in(value: &mut Value, source: &Path) -> Result<(), ConfigError> {
    match value {
        Value::String(s) => {
            *s = expand_string(s, source)?;
        }
        Value::Table(table) => {
            for (_, inner) in table.iter_mut() {
                expand_env_in(inner, source)?;
            }
        }
        Value::Array(items) => {
            for inner in items.iter_mut() {
                expand_env_in(inner, source)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Substitute every `${NAME}` in `s` with that environment variable's value.
///
/// A literal `$` that is not followed by `{` is preserved. An unterminated
/// `${` (no closing `}`) is also an error, because the alternative - silently
/// dropping it - leaves the file's contract unstated.
fn expand_string(s: &str, source: &Path) -> Result<String, ConfigError> {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '$' {
            out.push(c);
            continue;
        }
        if chars.peek() != Some(&'{') {
            // Literal `$` not followed by `{`. Preserved verbatim so a path
            // like `$HOME/x` survives rather than vanishing the `$`.
            out.push(c);
            continue;
        }
        chars.next();
        let mut name = String::new();
        let mut closed = false;
        for next in chars.by_ref() {
            if next == '}' {
                closed = true;
                break;
            }
            name.push(next);
        }
        if !closed {
            return Err(ConfigError::Parse(
                source.to_path_buf(),
                format!("unterminated `${{` in `{s}`"),
            ));
        }
        let value = env::var(&name)
            .map_err(|_| ConfigError::EnvVarUnset(name.clone(), source.display().to_string()))?;
        out.push_str(&value);
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
