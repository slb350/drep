//! Render and write `drep.toml`.
//!
//! The on-disk shape is fixed by Phase 5b: every entry has its documented
//! position, comments explain *why* a field exists (or doesn't), and the file
//! parses cleanly through [`crate::config::load`] once the referenced env
//! vars are set. `init` and `config` agreeing about the file is the whole
//! reason `[[llm]]` lands in this phase rather than in 5c.
//!
//! Escaping happens here rather than at a higher level, so `render` is the
//! single place a caller's value could fail to escape. See [`escape`] for what
//! TOML actually requires - it is more than the two characters this originally
//! handled.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};

use super::presets::{LlmPreset, PresetBackend};
use crate::llm::quirks::Quirks;

/// Fields used by exactly one execution backend.
#[derive(Debug, Clone)]
enum ChoiceBackend {
    Http {
        endpoint: String,
        key_in_store: bool,
        quirks: Quirks,
    },
    Codex,
}

/// One provider the user chose, ready to render.
///
/// `endpoint` and `model` are resolved rather than optional: the wizard and the
/// flag path both fall back to the preset's defaults before building this, so
/// the renderer never has to decide what "no model" means.
#[derive(Debug, Clone)]
pub struct Choice {
    /// The preset this came from - it supplies the protocol, the timeout and
    /// the environment variable name.
    pub preset: &'static LlmPreset,
    /// The model to ask for.
    pub model: String,
    /// Backend-specific values, already resolved by the wizard or flag path.
    backend: ChoiceBackend,
}

impl Choice {
    pub fn http(
        preset: &'static LlmPreset,
        model: String,
        endpoint: String,
        key_in_store: bool,
        quirks: Quirks,
    ) -> Self {
        assert!(
            matches!(preset.backend, PresetBackend::Http(_)),
            "HTTP choice requires an HTTP preset"
        );
        Self {
            preset,
            model,
            backend: ChoiceBackend::Http {
                endpoint,
                key_in_store,
                quirks,
            },
        }
    }

    pub fn codex(preset: &'static LlmPreset, model: String) -> Self {
        assert!(
            matches!(preset.backend, PresetBackend::Codex(_)),
            "Codex choice requires a Codex preset"
        );
        Self {
            preset,
            model,
            backend: ChoiceBackend::Codex,
        }
    }

    pub fn is_http(&self) -> bool {
        matches!(self.backend, ChoiceBackend::Http { .. })
    }

    pub fn endpoint(&self) -> Option<&str> {
        match &self.backend {
            ChoiceBackend::Http { endpoint, .. } => Some(endpoint),
            ChoiceBackend::Codex => None,
        }
    }

    pub fn key_in_store(&self) -> bool {
        match &self.backend {
            ChoiceBackend::Http { key_in_store, .. } => *key_in_store,
            ChoiceBackend::Codex => false,
        }
    }

    pub fn quirks(&self) -> Quirks {
        match &self.backend {
            ChoiceBackend::Http { quirks, .. } => *quirks,
            ChoiceBackend::Codex => Quirks {
                temperature: None,
                max_tokens: None,
                max_tokens_from_registry: false,
            },
        }
    }
}

/// Render the whole `drep.toml`, comments included, for a failover chain.
///
/// Produces exactly the shape [`crate::config::load`] accepts: one `[[llm]]`
/// table per choice in order, the keys in the order they appear in the file (the
/// TOML document parser preserves order; serde reads them positionally), and
/// each optional line only when the choice calls for it.
///
/// The order is the chain: entry one is tried first and each later entry is a
/// fallback for the one before it.
pub fn render_chain(choices: &[Choice]) -> String {
    let mut body = String::new();
    body.push_str("# drep configuration, written by `drep init`.\n");
    body.push_str("#\n");
    body.push_str("# Providers are declared as `[[llm]]`, an ordered array of tables: a\n");
    body.push_str("# preference order. Each one is tried in turn, and a transport failure -\n");
    body.push_str("# unreachable, timed out, rate limited, 5xx, or an empty answer - falls\n");
    body.push_str("# through to the next. A 401 or 403 does not: that is a broken key, and\n");
    body.push_str("# failing over would hide it. Add a fallback by adding another block:\n");
    body.push_str("#\n");
    body.push_str("#     [[llm]]\n");
    body.push_str("#     endpoint = \"https://openrouter.ai/api/v1\"\n");
    body.push_str("#     model = \"deepseek/deepseek-v4-pro-0813\"\n");
    body.push_str("#\n");
    body.push_str("# Set `enabled = false` on a block to park it without deleting it.\n");
    body.push_str("#\n");
    if choices.iter().any(Choice::is_http) {
        body.push_str(
            "# API keys are NOT in this file. `drep init` stores them per machine, keyed\n",
        );
        body.push_str("# by endpoint, so this file carries only the provider choice and can be\n");
        body.push_str("# committed. `drep auth list` shows what is stored. To pin a key to a\n");
        body.push_str(
            "# variable instead - which is what CI wants - add `api_key = \"${VAR}\"` to\n",
        );
        body.push_str("# a block; an explicit value always wins over the stored one.\n");
    }
    if choices.iter().any(|choice| !choice.is_http()) {
        body.push_str("# Codex owns ChatGPT subscription login and token refresh.\n");
        body.push_str("# Run `codex login`; drep never reads or stores those credentials.\n");
    }

    for choice in choices {
        body.push('\n');
        render_one(&mut body, choice);
    }

    body
}

/// Render one `[[llm]]` block into `body`.
fn render_one(body: &mut String, choice: &Choice) {
    let preset = choice.preset;

    body.push_str("[[llm]]\n");
    body.push_str("enabled = true\n");
    if let (PresetBackend::Codex(codex), ChoiceBackend::Codex) = (&preset.backend, &choice.backend)
    {
        body.push_str("backend = \"codex\"\n");
        body.push_str(&format!("model = \"{}\"\n", escape(&choice.model)));
        if let Some(effort) = &codex.reasoning_effort {
            body.push_str(&format!("reasoning_effort = \"{}\"\n", effort.as_str()));
        }
        if let Some(timeout) = preset.timeout_secs {
            body.push_str(&format!("timeout_secs = {timeout}\n"));
        }
        body.push_str(&format!("max_concurrent = {}\n", codex.max_concurrent));
        body.push_str(
            "# Reviews consume ChatGPT/Codex subscription allowance, not OpenAI API billing.\n",
        );
        return;
    }

    let (http, endpoint, key_in_store, quirks) = match (&preset.backend, &choice.backend) {
        (
            PresetBackend::Http(http),
            ChoiceBackend::Http {
                endpoint,
                key_in_store,
                quirks,
            },
        ) => (http, endpoint.as_str(), *key_in_store, quirks),
        _ => panic!("choice backend does not match preset `{}`", preset.key),
    };
    body.push_str(&format!("endpoint = \"{}\"\n", escape(endpoint)));
    body.push_str(&format!("model = \"{}\"\n", escape(&choice.model)));

    // Omitted entirely when the key is in the store: an explicit `api_key` wins
    // over a stored one, so writing `${VAR}` here would override the key
    // `drep init` just saved with a variable nobody set.
    if !key_in_store && let Some(env) = http.api_key_env {
        body.push_str(&format!("api_key = \"${{{env}}}\"\n"));
    }

    // Written only when the preset names one, so an OpenAI-compatible block keeps
    // the shape it has had since 2.0 and no existing file has to be migrated.
    if let Some(protocol) = http.protocol {
        body.push_str("# This endpoint speaks Anthropic's messages API, not chat completions.\n");
        body.push_str(&format!("protocol = \"{}\"\n", escape(protocol)));
    }

    // Written only for an endpoint that refuses a request without it. Everywhere
    // else an unset cap is what stops a reasoning model being truncated mid-thought.
    //
    // The second comment line says where the number came from, because "this is
    // the model's own limit" is a claim in a file the user commits, and it is
    // false whenever the registry could not name the model.
    if let Some(max_tokens) = quirks.max_tokens {
        body.push_str("# Required by this endpoint: it refuses a request that omits the field.\n");
        if quirks.max_tokens_from_registry {
            body.push_str(
                "# This is the model's own published output limit, not a cap drep chose.\n",
            );
        } else {
            body.push_str(
                "# This model's own limit is not known here, so it is the provider's fallback:\n\
                 # set well above any review-sized response.\n",
            );
        }
        body.push_str(&format!("max_tokens = {max_tokens}\n"));
    }

    // Absent means the parameter is omitted from the request entirely, which is what
    // a model that rejects it requires. That is a property of the model, so the
    // chosen model decides rather than the file inheriting a default.
    match quirks.temperature {
        Some(temperature) => {
            // `{:?}` rather than `{}`: Display renders `1.0` as `1`, which TOML
            // reads as an *integer* and `config::load` then refuses with a type
            // error - from a file `drep init` had just reported writing.
            body.push_str(&format!("temperature = {temperature:?}\n"));
        }
        None => {
            body.push_str("# `temperature` is deliberately absent: this model rejects the\n");
            body.push_str("# parameter, and the resulting 400 neither fails over nor retries.\n");
        }
    }

    if let Some(timeout) = preset.timeout_secs {
        body.push_str(
            "# A reasoning model can spend minutes on one file; the wall clock has to match.\n",
        );
        body.push_str(&format!("timeout_secs = {timeout}\n"));
    }

    if quirks.max_tokens.is_none() {
        body.push_str(
            "# max_tokens is deliberately unset: with no completion cap, a reasoning model\n",
        );
        body.push_str("# is never truncated mid-thought. Set it only to cap spend.\n");
    }
}

/// The refusal to overwrite an existing config.
///
/// Shared with `init::existing_config`, which has to make the same decision
/// *before* the wizard asks anything - the wizard stores a pasted key on its
/// way through, so refusing at the write half-applies the run. Two copies of
/// the message meant two places to keep the `--force` instruction in step.
pub fn already_exists(path: &Path) -> anyhow::Error {
    anyhow!(
        "{} already exists. Re-run with --force to replace it.",
        path.display()
    )
}

/// Escape a string for a TOML basic string.
///
/// `\` and `"` are the obvious two, and were once the only two handled here -
/// which was wrong: TOML forbids the literal control characters U+0000-U+0008,
/// U+000A-U+001F and U+007F inside a basic string. A model or endpoint
/// carrying a stray `\r` (a URL pasted from a CRLF file is the realistic way
/// in) produced a `drep.toml` that `config::load` then refused to parse - so
/// `drep init` reported success and left behind a config nothing could read,
/// and `write` would not replace it without `--force`.
///
/// The characters with short escapes get them (tab included - legal literally,
/// but clearer escaped in a file a human reads); anything else in the control
/// range becomes `\uXXXX`.
fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            // The rest of the control range has no short form.
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", c as u32)),
            other => out.push(other),
        }
    }
    out
}

/// Write `drep.toml` under `root`.
///
/// Refuses to overwrite an existing file unless `force`. The error names the
/// path and points at `--force` so the user does not have to read code to
/// learn how to recover.
pub fn write(root: &Path, body: &str, force: bool) -> Result<PathBuf> {
    use std::io::Write;

    let path = root.join(crate::config::default_config_path());

    // `create_new` rather than `exists()` then `write`. Two reasons, and the
    // second is the one that bites: the check and the write are not one
    // operation, and `Path::exists` reports *false* for a dangling symlink
    // while `fs::write` follows it - so a `drep.toml` symlinked at something
    // that does not exist yet got the config written through it, to a path
    // nobody named. `create_new` asks the OS the question and does the write
    // in the same call, and refuses a symlink outright.
    let mut options = std::fs::OpenOptions::new();
    options.write(true);
    if force {
        options.create(true).truncate(true);
    } else {
        options.create_new(true);
    }

    let mut file = match options.open(&path) {
        Ok(file) => file,
        Err(err) if !force && err.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err(already_exists(&path));
        }
        Err(err) => {
            return Err(
                anyhow::Error::new(err).context(format!("could not write {}", path.display()))
            );
        }
    };
    file.write_all(body.as_bytes())
        .with_context(|| format!("could not write {}", path.display()))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_write_that_fails_for_another_reason_is_not_reported_as_already_existing() {
        // `already_exists` names `--force` as the way out, so reporting it for
        // a missing directory or a permission error sends the user to a flag
        // that cannot help - and `--force` would then fail the same way with
        // the same message.
        let dir = tempfile::tempdir().expect("tempdir");
        let missing = dir.path().join("no-such-directory");

        let err = write(&missing, "model = \"x\"\n", false)
            .expect_err("there is no directory to write into");

        assert!(
            !err.to_string().contains("already exists"),
            "nothing exists here; got {err}"
        );
        assert!(err.to_string().contains("could not write"), "got {err}");
    }

    #[cfg(unix)]
    #[test]
    fn a_dangling_symlink_named_drep_toml_is_refused_rather_than_followed() {
        // `Path::exists` reports false for a symlink whose target is missing,
        // while `fs::write` creates the target - so the guard said "nothing is
        // there" and the write went somewhere nobody named.
        let dir = tempfile::tempdir().expect("tempdir");
        let elsewhere = dir.path().join("elsewhere.toml");
        std::os::unix::fs::symlink(&elsewhere, dir.path().join("drep.toml")).expect("symlink");

        let err = write(dir.path(), "model = \"x\"\n", false)
            .expect_err("a dangling symlink is still something being there");

        assert!(err.to_string().contains("drep.toml"), "got {err}");
        assert!(
            !elsewhere.exists(),
            "and nothing was written through it to {}",
            elsewhere.display()
        );
    }

    #[test]
    fn escape_handles_backslash_and_quote() {
        assert_eq!(escape("plain"), "plain");
        assert_eq!(escape(r#"a"b"#), r#"a\"b"#);
        assert_eq!(escape(r"a\b"), r"a\\b");
        assert_eq!(escape(r#"a"b\c"#), r#"a\"b\\c"#);
    }
}
