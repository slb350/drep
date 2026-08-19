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

/// Render the whole `drep.toml`, comments included.
///
/// `render` produces exactly the shape [`crate::config::load`] accepts:
/// one `[[llm]]` table, the keys in the order they appear in the file (the
/// TOML document parser preserves order; serde reads them positionally),
/// and `api_key`/`timeout_secs` lines only when the preset calls for them.
pub fn render(preset: &LlmPreset, model: &str, endpoint: &str) -> String {
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
    body.push_str("#     api_key = \"${OPENROUTER_API_KEY}\"\n");
    body.push_str("#\n");
    body.push_str("# Set `enabled = false` on a block to park it without deleting it.\n");
    body.push_str("#\n");
    body.push_str("# `api_key` names an environment variable rather than holding the secret, so\n");
    body.push_str("# this file can be committed and the key cannot.\n");
    body.push('\n');

    body.push_str("[[llm]]\n");
    body.push_str("enabled = true\n");
    body.push_str(&format!("endpoint = \"{}\"\n", escape(endpoint)));
    body.push_str(&format!("model = \"{}\"\n", escape(model)));

    if let Some(env) = preset.api_key_env {
        body.push_str(&format!("api_key = \"${{{env}}}\"\n"));
    }

    if let Some(timeout) = preset.timeout_secs {
        body.push_str(
            "# A reasoning model can spend minutes on one file; the wall clock has to match.\n",
        );
        body.push_str(&format!("timeout_secs = {timeout}\n"));
    }

    body.push('\n');
    body.push_str(
        "# max_tokens is deliberately unset: with no completion cap, a reasoning model\n",
    );
    body.push_str("# is never truncated mid-thought. Set it only to cap spend.\n");

    body
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
        return Err(anyhow!(
            "{} already exists. Re-run with --force to replace it.",
            path.display()
        ));
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
