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

use super::presets::LlmPreset;

/// One provider the user chose, ready to render.
///
/// `endpoint` and `model` are resolved rather than optional: the wizard and the
/// flag path both fall back to the preset's defaults before building this, so
/// the renderer never has to decide what "no model" means.
#[derive(Debug, Clone)]
pub struct Choice {
    /// The preset this came from - it supplies the protocol, the ceilings and
    /// the environment variable name.
    pub preset: &'static LlmPreset,
    /// The model to ask for.
    pub model: String,
    /// The base URL.
    pub endpoint: String,
    /// Whether the key is held in the user-level auth store.
    ///
    /// When it is, **no `api_key` line is written at all**: the key is looked up
    /// by endpoint at run time. Writing `${VAR}` as well would name a variable
    /// the user never set, and an explicit `api_key` wins over the store - so
    /// the file would override the very key `drep init` had just saved.
    pub key_in_store: bool,
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
    body.push_str("# API keys are NOT in this file. `drep init` stores them per machine, keyed\n");
    body.push_str("# by endpoint, so this file carries only the provider choice and can be\n");
    body.push_str("# committed. `drep auth list` shows what is stored. To pin a key to a\n");
    body.push_str("# variable instead - which is what CI wants - add `api_key = \"${VAR}\"` to\n");
    body.push_str("# a block; an explicit value always wins over the stored one.\n");

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
    body.push_str(&format!("endpoint = \"{}\"\n", escape(&choice.endpoint)));
    body.push_str(&format!("model = \"{}\"\n", escape(&choice.model)));

    // Omitted entirely when the key is in the store: an explicit `api_key` wins
    // over a stored one, so writing `${VAR}` here would override the key
    // `drep init` just saved with a variable nobody set.
    if !choice.key_in_store
        && let Some(env) = preset.api_key_env
    {
        body.push_str(&format!("api_key = \"${{{env}}}\"\n"));
    }

    // Written only when the preset names one, so an OpenAI-compatible block keeps
    // the shape it has had since 2.0 and no existing file has to be migrated.
    if let Some(protocol) = preset.protocol {
        body.push_str("# This endpoint speaks Anthropic's messages API, not chat completions.\n");
        body.push_str(&format!("protocol = \"{}\"\n", escape(protocol)));
    }

    // Written only for an endpoint that refuses a request without it. Everywhere
    // else an unset cap is what stops a reasoning model being truncated mid-thought.
    if let Some(max_tokens) = preset.max_tokens {
        body.push_str("# Required by this endpoint: it refuses a request that omits the field.\n");
        body.push_str(
            "# Set well above any review-sized response, so it is not a ceiling in practice.\n",
        );
        body.push_str(&format!("max_tokens = {max_tokens}\n"));
    }

    // Absent means the parameter is omitted from the request entirely, which is what
    // a model that rejects it requires. That is a property of the model, so the
    // preset decides rather than the file inheriting a default.
    match preset.temperature {
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

    if preset.max_tokens.is_none() {
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
    let path = root.join(crate::config::default_config_path());
    if path.exists() && !force {
        return Err(already_exists(&path));
    }
    std::fs::write(&path, body).with_context(|| format!("could not write {}", path.display()))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_handles_backslash_and_quote() {
        assert_eq!(escape("plain"), "plain");
        assert_eq!(escape(r#"a"b"#), r#"a\"b"#);
        assert_eq!(escape(r"a\b"), r"a\\b");
        assert_eq!(escape(r#"a"b\c"#), r#"a\"b\\c"#);
    }
}
