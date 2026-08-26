//! `${VAR}` expansion, and the single definition of what counts as a reference.
//!
//! Split out of the parent for size, not for a change of contract: the parent
//! re-exports the three public functions at their original paths.
//!
//! Two rules live here and nowhere else. An unset variable is an error rather
//! than an empty string, because a silently-empty credential produces a
//! confusing 401 instead of a clear "that variable is not set". And the
//! reference *grammar* has exactly one definition, consulted by the substituter
//! and by `doctor`: `doctor` once carried a narrower regex, called a config
//! naming `${openrouter_key}` fine, and suppressed the real error believing it
//! had already reported it.

use std::collections::BTreeSet;
use std::env;
use std::path::Path;

use toml::Value;

use super::{ConfigError, LlmConfig};

/// The positions of the `[[llm]]` tables that carry `enabled = false`.
///
/// Read from the raw tree because expansion runs before deserialization - and
/// it has to, since an unset variable must be reported with the variable's name
/// rather than as a downstream parse failure inside the substituted text. The
/// default comes from `LlmConfig::default()` so this cannot disagree with serde
/// about what an absent `enabled` key means.
pub(super) fn disabled_provider_indices(tree: &Value) -> BTreeSet<usize> {
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
pub(super) fn expand_env_except(
    tree: &mut Value,
    source: &Path,
    skip: &BTreeSet<usize>,
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
pub(super) fn expand_env_in(value: &mut Value, source: &Path) -> Result<(), ConfigError> {
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
/// An unterminated `${` yields nothing here; `expand_string` is what reports
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

/// Every `${NAME}` reference that [`super::load`] will actually try to substitute.
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
    let mut seen = BTreeSet::new();
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
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    collect_env_refs(value, &mut seen, &mut out);
    out
}

fn collect_env_refs(value: &Value, seen: &mut BTreeSet<String>, out: &mut Vec<String>) {
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
                "unterminated environment variable reference `${`".to_owned(),
            ));
        }
        if name.is_empty() {
            return Err(ConfigError::Parse(
                source.to_path_buf(),
                "empty environment variable reference `${}`".to_owned(),
            ));
        }
        let value = match env::var(&name) {
            Ok(value) => value,
            Err(env::VarError::NotPresent) => {
                return Err(ConfigError::EnvVarUnset(name, source.display().to_string()));
            }
            Err(env::VarError::NotUnicode(_)) => {
                return Err(ConfigError::EnvVarNotUnicode(
                    name,
                    source.display().to_string(),
                ));
            }
        };
        out.push_str(&value);
    }
    Ok(out)
}
