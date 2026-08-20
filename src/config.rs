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
//!   confusing 401 instead of a clear "GITHUB_TOKEN is not set".
//! - **`max_tokens` defaults to None**, meaning no cap is sent to the model.
//!   Modern reasoning models ship 256k-1M context, and inventing a ceiling
//!   truncates them mid-thought. The option stays available for capping
//!   spend.

use std::env;
use std::path::{Path, PathBuf};

use open_agent::ApiProtocol;
use serde::Deserialize;
use thiserror::Error;
use toml::Value;

/// The whole configuration tree, rooted at the file.
///
/// `llm` is an **array of tables** (`[[llm]]`), not a single `[llm]` section,
/// and it is that shape from the day `drep init` first wrote one - a phase
/// before failover could read it, precisely so the file format would not have
/// to change underneath a file drep itself wrote. The list is a *preference
/// order*: [`Self::providers`] is the failover chain.
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

/// One provider in the failover chain.
///
/// Field ordering matches the spec; every field has its documented default
/// here so partial files work.
///
/// **`enabled` defaults to `true`** - it is an opt-*out*, the way to park one
/// entry of an ordered list without deleting it. It defaulted to `false` while
/// the list held exactly one consulted entry, which made declaring a provider
/// do nothing until you also enabled it: a user who added a fallback by
/// copying the first block minus its `enabled` line got a silently inert
/// entry and no failover. A partial entry that names no endpoint is still
/// rejected, by `LlmClient::new`, with a message about the endpoint rather
/// than about a switch the user never touched.
#[derive(Deserialize)]
#[serde(default)]
pub struct LlmConfig {
    pub enabled: bool,
    pub endpoint: Option<String>,
    pub model: Option<String>,
    pub api_key: Option<String>,
    /// The wire protocol the endpoint speaks: `"openai"` (the default) or
    /// `"anthropic"`. Held as the raw string so an unrecognised value can be
    /// reported by name; [`parse_protocol`] is the single definition of what
    /// the names mean, and it defers to the SDK's own parser rather than
    /// keeping a second table of them here.
    pub protocol: Option<String>,
    /// Sampling temperature, or `None` to omit the parameter entirely.
    ///
    /// **Unset means unset**, not 0.2. It defaulted to 0.2 while every
    /// endpoint drep could reach accepted the parameter; several now reject
    /// it outright - `k3` answers `only temperature 1 is allowed for this
    /// model` with a 400, which neither fails over nor retries - so "send no
    /// temperature" had to become expressible. A provider that wants a
    /// deterministic review says so, and `drep init` writes it for every
    /// preset whose model accepts one.
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub timeout_secs: u64,
    pub max_retries: u32,
    pub max_concurrent: usize,
}

/// Hand-written so the API key cannot reach a log.
///
/// A derived `Debug` prints every field, so any `{:?}`, `dbg!` or tracing line
/// touching a `Config` - which derives `Debug` and holds these - would emit a
/// live credential. `LlmClient` already redacts for exactly this reason; the
/// config the client is built *from* held the same secret in the clear.
impl std::fmt::Debug for LlmConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LlmConfig")
            .field("enabled", &self.enabled)
            .field("endpoint", &self.endpoint)
            .field("model", &self.model)
            .field(
                "api_key",
                &self
                    .api_key
                    .as_ref()
                    .map(|_| "<redacted>")
                    .unwrap_or("None"),
            )
            .field("protocol", &self.protocol)
            .field("temperature", &self.temperature)
            .field("max_tokens", &self.max_tokens)
            .field("timeout_secs", &self.timeout_secs)
            .field("max_retries", &self.max_retries)
            .field("max_concurrent", &self.max_concurrent)
            .finish()
    }
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            endpoint: None,
            model: None,
            api_key: None,
            protocol: None,
            temperature: None,
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

    /// Rejected rather than defaulted: falling back to `openai` would post
    /// chat-completions bytes to a `/messages` endpoint, and the resulting 404
    /// reads as "the provider is down" rather than "this line has a typo".
    #[error(
        "[[llm]] #{} in file order: unknown protocol `{value}`; expected `openai` or `anthropic`",
        index + 1
    )]
    UnknownProtocol { index: usize, value: String },

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

    // `toml::from_str::<Value>` and `<Value as FromStr>::from_str` are NOT
    // interchangeable in toml 1.x, despite producing the same type. The former
    // runs the document parser; the latter runs `ValueDeserializer`, which
    // parses a single TOML *value* (`42`, `"text"`) and rejects a whole document
    // with "unexpected content, expected nothing".
    let mut tree: Value = toml::from_str(&content).map_err(|err: toml::de::Error| {
        ConfigError::Parse(path.to_path_buf(), err.message().to_owned())
    })?;

    // Disabled providers are pruned from expansion, not from the tree: a
    // parked entry is inert, so an unset `${OPENROUTER_API_KEY}` in the cloud
    // block a user just switched off must not refuse to load the file. It stays
    // in `Config.llm` with its `${VAR}` unexpanded, which nothing reads - only
    // `providers()` is consulted, and it filters the entry out.
    let disabled = disabled_provider_indices(&tree);
    expand_env_except(&mut tree, path, &disabled)?;

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
        if llm.max_concurrent == 0 {
            return Err(ConfigError::ZeroConcurrency { index });
        }
    }
    Ok(())
}

/// The positions of the `[[llm]]` tables that carry `enabled = false`.
///
/// Read from the raw tree because expansion runs before deserialization - and
/// it has to, since an unset variable must be reported with the variable's name
/// rather than as a downstream parse failure inside the substituted text. The
/// default comes from `LlmConfig::default()` so this cannot disagree with serde
/// about what an absent `enabled` key means.
fn disabled_provider_indices(tree: &Value) -> std::collections::BTreeSet<usize> {
    let default_enabled = LlmConfig::default().enabled;
    tree.get("llm")
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .enumerate()
                .filter(|(_, entry)| {
                    !entry
                        .get("enabled")
                        .and_then(Value::as_bool)
                        .unwrap_or(default_enabled)
                })
                .map(|(index, _)| index)
                .collect()
        })
        .unwrap_or_default()
}

/// [`expand_env_in`] over the whole tree except the named `[[llm]]` entries.
fn expand_env_except(
    tree: &mut Value,
    source: &Path,
    skip: &std::collections::BTreeSet<usize>,
) -> Result<(), ConfigError> {
    if skip.is_empty() {
        return expand_env_in(tree, source);
    }
    let Some(table) = tree.as_table_mut() else {
        return expand_env_in(tree, source);
    };
    for (key, value) in table.iter_mut() {
        if key != "llm" {
            expand_env_in(value, source)?;
            continue;
        }
        let Some(entries) = value.as_array_mut() else {
            expand_env_in(value, source)?;
            continue;
        };
        for (index, entry) in entries.iter_mut().enumerate() {
            if !skip.contains(&index) {
                expand_env_in(entry, source)?;
            }
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

/// Every `${NAME}` reference in `s`, in the order they appear.
///
/// The single statement of what counts as a variable reference, so a consumer
/// cannot disagree with the substituter about it. `drep doctor` had its own
/// regex, `\$\{([A-Z_][A-Z0-9_]*)\}`, which is *narrower* than this: a config
/// naming `${openrouter_key}` produced no warning from doctor, while
/// `expand_string` below still failed on it — and doctor suppressed that error
/// believing it had already reported it. The user was told the config was fine
/// and `drep check` then refused to load it.
///
/// An unterminated `${` yields nothing here; [`expand_string`] is what reports
/// it, because only the substituter knows it is an error rather than literal
/// text.
pub fn env_var_refs(s: &str) -> Vec<String> {
    let mut refs = Vec::new();
    let mut rest = s;
    // Written with `split_once`/`strip_prefix` rather than `find` plus index
    // arithmetic. The arithmetic version was correct, but `start + 2` and
    // `end + 1` are two magic offsets whose only justification is the length
    // of the delimiters they skip - and the delimiters are right there in the
    // pattern, so letting the standard library consume them says the same
    // thing without the chance of an off-by-one.
    while let Some((_, after_open)) = rest.split_once("${") {
        let Some((name, after_close)) = after_open.split_once('}') else {
            // An unterminated `${`. Not this function's error to report:
            // `expand_string` is what knows whether the text is a reference or
            // a literal, and it rejects the file.
            break;
        };
        refs.push(name.to_owned());
        rest = after_close;
    }
    refs
}

/// Every `${NAME}` reference that [`load`] will actually try to substitute.
///
/// The same tree as [`env_var_refs_in`], minus the `[[llm]]` entries that
/// carry `enabled = false` — because `load` skips expanding those, so a
/// variable named only by a parked provider is not required and reporting it
/// as missing is a false alarm. This is the shared definition, so `doctor`
/// cannot warn about a variable `check` does not need; a narrower scanner in
/// `doctor` is what once made it call a config fine that `check` refused to
/// load.
pub fn required_env_var_refs(value: &Value) -> Vec<String> {
    let disabled = disabled_provider_indices(value);
    if disabled.is_empty() {
        return env_var_refs_in(value);
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut out = Vec::new();
    let Some(table) = value.as_table() else {
        return env_var_refs_in(value);
    };
    for (key, inner) in table {
        if key != "llm" {
            collect_env_refs(inner, &mut seen, &mut out);
            continue;
        }
        let Some(entries) = inner.as_array() else {
            collect_env_refs(inner, &mut seen, &mut out);
            continue;
        };
        for (index, entry) in entries.iter().enumerate() {
            if !disabled.contains(&index) {
                collect_env_refs(entry, &mut seen, &mut out);
            }
        }
    }
    out
}

/// Every `${NAME}` reference in any string value of a parsed TOML tree.
///
/// Deliberately over the *parsed* tree rather than the file text: a `${VAR}`
/// inside a comment is documentation, not a reference, and reporting it as an
/// unset variable is a false alarm in the one command whose job is to be
/// believed. Deduplicated, first-seen order preserved.
pub fn env_var_refs_in(value: &Value) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
    let mut out = Vec::new();
    collect_env_refs(value, &mut seen, &mut out);
    out
}

fn collect_env_refs(
    value: &Value,
    seen: &mut std::collections::BTreeSet<String>,
    out: &mut Vec<String>,
) {
    match value {
        Value::String(s) => {
            for name in env_var_refs(s) {
                if seen.insert(name.clone()) {
                    out.push(name);
                }
            }
        }
        Value::Table(table) => {
            for (_, inner) in table {
                collect_env_refs(inner, seen, out);
            }
        }
        Value::Array(items) => {
            for inner in items {
                collect_env_refs(inner, seen, out);
            }
        }
        _ => {}
    }
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
