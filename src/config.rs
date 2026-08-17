//! TOML configuration.
//!
//! One file per repository, conventionally `drep.toml` at the root. The shape
//! is deliberate: every field has a documented default, so a partial file
//! works. Missing keys are not an error - the section just inherits the
//! default.
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
/// Sections are added as the surface widens. `#[serde(default)]` means a file
/// with an empty body deserializes successfully - the user opted into "all
/// defaults" by writing the file.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    pub llm: LlmConfig,
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

    #[error("temperature {0} is outside the allowed range 0.0..=2.0")]
    Temperature(f32),
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

    validate(&config)?;
    Ok(config)
}

/// Validate fields that serde cannot enforce from the type alone.
fn validate(config: &Config) -> Result<(), ConfigError> {
    let t = config.llm.temperature;
    if !(0.0..=2.0).contains(&t) {
        return Err(ConfigError::Temperature(t));
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
mod tests {
    use super::*;
    use std::fs;

    /// `expand_env_in` walks arrays as well as tables.
    ///
    /// No field in `LlmConfig` is an array today, so this cannot be reached
    /// through `load()` — which is exactly why the array arm survived
    /// mutation testing. It is still load-bearing: the walk exists so a future
    /// array-valued field inherits `${VAR}` expansion without anyone
    /// remembering to opt in, and a silently-skipped arm would break that
    /// promise the moment such a field is added.
    #[test]
    fn env_expansion_descends_into_arrays() {
        // SAFETY: single-threaded test process; no other thread reads env here.
        unsafe { env::set_var("DREP_ARRAY_PROBE", "expanded") };

        let mut tree: Value = toml::from_str(
            r#"
values = ["${DREP_ARRAY_PROBE}", "literal"]
nested = { inner = ["${DREP_ARRAY_PROBE}"] }
"#,
        )
        .expect("fixture parses");

        expand_env_in(&mut tree, Path::new("probe.toml")).expect("expansion succeeds");

        let values = tree["values"].as_array().expect("array");
        assert_eq!(values[0].as_str(), Some("expanded"));
        assert_eq!(values[1].as_str(), Some("literal"));

        let inner = tree["nested"]["inner"].as_array().expect("nested array");
        assert_eq!(
            inner[0].as_str(),
            Some("expanded"),
            "arrays nested inside tables must expand too"
        );
    }

    /// Write `body` to a fresh `drep.toml`-shaped path in `temp`.
    fn write_config(temp: &tempfile::TempDir, body: &str) -> PathBuf {
        let path = temp.path().join("drep.toml");
        fs::write(&path, body).expect("write config");
        path
    }

    #[test]
    fn full_toml_round_trips_into_config_with_every_field_correct() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = write_config(
            &temp,
            r#"
[llm]
enabled = true
endpoint = "http://localhost:11434/v1"
model = "qwen3:8b"
api_key = "literal-secret"
temperature = 0.7
max_tokens = 4096
timeout_secs = 120
max_retries = 5
max_concurrent = 8
"#,
        );

        let config = load(&path).expect("load");
        let llm = &config.llm;
        assert!(llm.enabled);
        assert_eq!(llm.endpoint.as_deref(), Some("http://localhost:11434/v1"));
        assert_eq!(llm.model.as_deref(), Some("qwen3:8b"));
        assert_eq!(llm.api_key.as_deref(), Some("literal-secret"));
        assert!((llm.temperature - 0.7).abs() < f32::EPSILON);
        assert_eq!(llm.max_tokens, Some(4096));
        assert_eq!(llm.timeout_secs, 120);
        assert_eq!(llm.max_retries, 5);
        assert_eq!(llm.max_concurrent, 8);
    }

    #[test]
    fn partial_file_uses_documented_defaults_and_max_tokens_is_none() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = write_config(
            &temp,
            r#"
[llm]
enabled = true
model = "qwen3:8b"
"#,
        );

        let config = load(&path).expect("load");
        let llm = &config.llm;
        assert!(llm.enabled);
        assert_eq!(llm.model.as_deref(), Some("qwen3:8b"));
        assert!(llm.endpoint.is_none());
        assert!(llm.api_key.is_none());
        assert!((llm.temperature - 0.2).abs() < f32::EPSILON, "default 0.2");
        assert_eq!(llm.max_tokens, None, "absent max_tokens is None, not 0");
        assert_eq!(llm.timeout_secs, 60, "default timeout");
        assert_eq!(llm.max_retries, 3, "default max_retries");
        assert_eq!(llm.max_concurrent, 3, "default max_concurrent");
    }

    #[test]
    fn temperature_outside_range_is_rejected() {
        for bad in ["-0.1", "2.5", "100.0"] {
            let temp = tempfile::tempdir().expect("tempdir");
            let path = write_config(
                &temp,
                &format!(
                    r#"
[llm]
temperature = {bad}
"#
                ),
            );
            let err = load(&path).expect_err("should reject");
            assert!(
                matches!(err, ConfigError::Temperature(_)),
                "expected Temperature error, got {err:?} for value {bad}"
            );
        }
    }

    #[test]
    fn env_var_in_api_key_expands_from_environment() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = write_config(
            &temp,
            r#"
[llm]
api_key = "${DREP_TEST_API_KEY_VAR}"
"#,
        );

        // Use a name unique to this test so a parallel run or a leaked env
        // var from elsewhere cannot poison the assertion.
        let var = "DREP_TEST_API_KEY_VAR";
        // SAFETY: the test is single-threaded at this point and the var name
        // is unique to this test. We restore on either branch to keep the
        // surrounding tests deterministic.
        let previous = env::var(var).ok();
        // SAFETY: see above.
        unsafe {
            env::set_var(var, "expanded-secret-value");
        }
        let result = load(&path);
        if let Some(prev) = previous {
            unsafe {
                env::set_var(var, prev);
            }
        } else {
            unsafe {
                env::remove_var(var);
            }
        }

        let config = result.expect("load");
        assert_eq!(config.llm.api_key.as_deref(), Some("expanded-secret-value"));
    }

    #[test]
    fn env_var_with_unset_variable_is_an_error_not_an_empty_string() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = write_config(
            &temp,
            r#"
[llm]
api_key = "${DREP_DEFINITELY_NOT_SET_VAR_XYZ_123}"
"#,
        );

        // Make sure no other test or shell leaked the variable.
        let previous = env::var("DREP_DEFINITELY_NOT_SET_VAR_XYZ_123").ok();
        unsafe {
            env::remove_var("DREP_DEFINITELY_NOT_SET_VAR_XYZ_123");
        }
        let result = load(&path);
        if let Some(prev) = previous {
            unsafe {
                env::set_var("DREP_DEFINITELY_NOT_SET_VAR_XYZ_123", prev);
            }
        }

        let err = result.expect_err("unset variable must fail");
        match err {
            ConfigError::EnvVarUnset(name, _) => {
                assert_eq!(name, "DREP_DEFINITELY_NOT_SET_VAR_XYZ_123");
            }
            other => panic!("expected EnvVarUnset, got {other:?}"),
        }
    }

    #[test]
    fn literal_api_key_passes_through_unchanged() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = write_config(
            &temp,
            r#"
[llm]
api_key = "this-is-not-a-template"
"#,
        );

        let config = load(&path).expect("load");
        assert_eq!(
            config.llm.api_key.as_deref(),
            Some("this-is-not-a-template")
        );
    }

    #[test]
    fn literal_dollar_not_followed_by_brace_is_preserved() {
        // The expansion rule is `${VAR}` exactly. A bare `$5` or `$HOME`
        // (without braces) must survive so filenames and shell syntax stay
        // intact.
        let temp = tempfile::tempdir().expect("tempdir");
        let path = write_config(
            &temp,
            r#"
[llm]
endpoint = "http://host/$1/path"
"#,
        );

        let config = load(&path).expect("load");
        assert_eq!(config.llm.endpoint.as_deref(), Some("http://host/$1/path"));
    }

    #[test]
    fn missing_file_path_is_an_error() {
        let temp = tempfile::tempdir().expect("tempdir");
        let missing = temp.path().join("does_not_exist.toml");
        let err = load(&missing).expect_err("missing file must error");
        assert!(
            matches!(err, ConfigError::Io(_, _)),
            "expected Io error, got {err:?}"
        );
    }

    #[test]
    fn max_tokens_absent_yields_none_and_present_yields_some() {
        let temp = tempfile::tempdir().expect("tempdir");
        let absent = write_config(&temp, "[llm]\nmodel = \"x\"\n");
        let config = load(&absent).expect("load");
        assert_eq!(config.llm.max_tokens, None);

        let present = write_config(&temp, "[llm]\nmax_tokens = 8192\n");
        let config = load(&present).expect("load");
        assert_eq!(config.llm.max_tokens, Some(8192));
    }

    #[test]
    fn default_config_path_is_drep_toml_in_cwd() {
        assert_eq!(default_config_path(), PathBuf::from("drep.toml"));
    }
}
