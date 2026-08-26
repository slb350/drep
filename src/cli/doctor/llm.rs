//! The `LLM analysis (required):` block of `drep doctor`.
//!
//! Split out of the parent for size, not for a change of contract. Two rules
//! govern everything here:
//!
//! - **The listing describes the file the user wrote.** Model, endpoint,
//!   protocol and key source are read from the *raw* TOML tree, so a `${VAR}`
//!   prints as itself rather than being swallowed by the variable-not-set
//!   error. A fresh clone with nothing exported is exactly when this report is
//!   most useful.
//! - **The probes describe what will actually run.** A credential helper is
//!   invoked with the *expanded* argv, because reporting on a command that still
//!   contains a literal `${VAULT_REF}` would report on a command `check` never
//!   runs.
//!
//! Nothing here ever gates. A broken provider, a missing key or a failing helper
//! is a diagnosis; `drep check` is the gate.

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::Result;
use toml::Value;

use crate::auth::{AuthStore, Declared, KeySource};
use crate::config;

use super::DoctorArgs;

/// `LLM analysis (required):` block.
///
/// Display path is the *raw* file, not `config::load`: a fresh clone is
/// exactly when the report is most useful, and `load` fails on an unset
/// referenced variable. `load` is consulted only to surface problems that are
/// not the unset variable, and to supply the expanded argv the credential probe
/// needs.
pub(super) async fn write_llm_section<W: Write>(
    out: &mut W,
    args: &DoctorArgs,
    root: &Path,
    auth_path: &Path,
    codex_probe: &dyn Fn() -> Result<crate::llm::codex::CodexStatus, String>,
) -> Result<()> {
    writeln!(out)?;
    writeln!(out, "LLM analysis (required):")?;

    let config_path: PathBuf = match &args.config {
        Some(p) => p.clone(),
        None => root.join(config::default_config_path()),
    };

    let raw = match std::fs::read_to_string(&config_path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            writeln!(
                out,
                "  No config file at {} - `drep check` cannot run. Run `drep init`.",
                config_path.display()
            )?;
            return Ok(());
        }
        Err(err) => {
            writeln!(out, "  {} could not be read: {err}", config_path.display())?;
            return Ok(());
        }
    };

    let value: toml::Value = match toml::from_str(&raw) {
        Ok(v) => v,
        Err(err) => {
            writeln!(
                out,
                "  {} could not be parsed: {}",
                config_path.display(),
                err.message()
            )?;
            return Ok(());
        }
    };

    // The three cases are distinguished, not collapsed. Folding "not an array"
    // into "absent" reports a type mismatch as "declares no `[[llm]]`
    // provider" and returns early, skipping the `config::load` check that names
    // the real problem and pointing at a command that will not overwrite it.
    let providers = match value.get("llm") {
        None => {
            writeln!(
                out,
                "  {} declares no `[[llm]]` provider. Run `drep init`.",
                config_path.display()
            )?;
            return Ok(());
        }
        Some(Value::Array(entries)) if entries.is_empty() => {
            writeln!(
                out,
                "  {} declares no `[[llm]]` provider. Run `drep init`.",
                config_path.display()
            )?;
            return Ok(());
        }
        Some(Value::Array(entries)) => entries,
        Some(_) => {
            writeln!(
                out,
                "  {} has an `llm` key that is not a `[[llm]]` array of tables. \
                 Providers are declared as `[[llm]]`, one block per provider.",
                config_path.display()
            )?;
            // Fall through to `config::load` below, which names the parse
            // error precisely.
            report_load_failure(out, &config_path)?;
            return Ok(());
        }
    };

    // Loaded once, and consumed twice: the credential probe needs the expanded
    // argv, and the tail names why the file will not load. Loading again at the
    // end would let the two disagree about the same file within one report.
    let loaded = config::load(&config_path);

    // Print providers verbatim from the raw file: model and endpoint come out
    // unexpanded, so `${VAR}` shows as `${VAR}` rather than being swallowed by
    // the variable-not-set error.
    //
    // A disabled entry is called out rather than listed as if it were in play.
    // Before failover existed this section listed every `[[llm]]` block without
    // noting that only one was ever consulted; now that the list is a real
    // failover chain, an inert entry is the one thing the listing can still get
    // wrong - a user who parks their local model wants to see that the cloud
    // entry below it is what will run, and a user who copied a block without
    // its `enabled` line wants to see that it is not.
    //
    // The numbering is the **chain position**, not the position in the file, so
    // a disabled entry gets a bullet rather than a number and the entries after
    // it shift up. That is what makes it agree with `drep check`: a failure
    // line reading "[1] cloud-model" has to name the same provider this listing
    // calls 1, and numbering the file would make the two disagree the moment
    // anything above was parked.
    // Read once for the whole listing. A store that cannot be read is reported
    // rather than fatal: `doctor` exists to describe a broken setup, so failing
    // out here would suppress everything else it had to say.
    let needs_auth_store = providers.iter().any(|entry| {
        entry_is_enabled(entry) && entry.get("backend").and_then(Value::as_str) != Some("codex")
    });
    let store = match needs_auth_store
        .then(|| AuthStore::load(auth_path))
        .transpose()
    {
        Ok(Some(store)) => store,
        Ok(None) => AuthStore::new(),
        Err(err) => {
            writeln!(out, "  The auth store could not be read: {err}")?;
            AuthStore::new()
        }
    };

    let mut enabled_count = 0usize;
    let mut codex_status: Option<Result<crate::llm::codex::CodexStatus, String>> = None;
    for (file_index, entry) in providers.iter().enumerate() {
        let model = entry
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or("(no model set)");
        let endpoint = entry
            .get("endpoint")
            .and_then(|v| v.as_str())
            .unwrap_or("(no endpoint set)");
        // Shown only when it is not the default, so an OpenAI-compatible listing
        // keeps the line it has always had. It is worth showing at all because
        // the protocol decides the path a request is posted to, and a wrong one
        // reports as the endpoint being down.
        let protocol = match entry.get("protocol").and_then(|v| v.as_str()) {
            None | Some("openai") => String::new(),
            Some(other) => format!(" [{other}]"),
        };
        let is_codex = entry.get("backend").and_then(Value::as_str) == Some("codex");
        let description = if is_codex {
            format!("{model} via ChatGPT/Codex subscription")
        } else {
            format!("{model} at {endpoint}{protocol}")
        };
        if entry_is_enabled(entry) {
            enabled_count += 1;
            writeln!(out, "  {enabled_count}. {description}")?;
            if is_codex {
                let status = codex_status.get_or_insert_with(codex_probe);
                match status {
                    Ok(status) => {
                        writeln!(out, "     Codex CLI: {}", status.cli_version())?;
                        writeln!(out, "     authentication: ChatGPT-managed")?;
                        writeln!(out, "     isolation: ephemeral, read-only, tools disabled")?;
                    }
                    Err(err) => writeln!(out, "     unavailable: {err}")?,
                }
            } else {
                let line =
                    key_source_line(entry, expanded_command(&loaded, file_index), &store).await;
                writeln!(out, "     key: {line}")?;
            }
        } else {
            writeln!(out, "  -  {description} (disabled - skipped)")?;
        }
    }
    writeln!(out, "  {}", failover_line(enabled_count))?;

    // Unset environment variables, deduped in first-seen order.
    //
    // Over the *parsed* tree, using `config`'s own scanner. Doctor had its own
    // regex - `\$\{([A-Z_][A-Z0-9_]*)\}` - which is narrower than what
    // `config::load` actually substitutes, so `${openrouter_key}` produced no
    // warning here while `load` still failed on it. And since the branch below
    // suppresses `EnvVarUnset` on the grounds it was "already reported", the
    // user got a clean-looking report for a config `drep check` refuses to
    // load. Scanning the parsed tree rather than the file text also stops a
    // `${VAR}` inside a comment raising a false alarm.
    for name in unset_env_vars(&value) {
        writeln!(
            out,
            "  {name} is NOT set - LLM analysis will fail until you export it."
        )?;
    }

    // Surface other load failures. `EnvVarUnset` is already reported above;
    // repeating it reads as two separate problems.
    match loaded {
        Err(config::ConfigError::EnvVarUnset(_, _)) => Ok(()),
        other => report_load_result(out, &config_path, other),
    }
}

/// The `api_key_command` argv `check` would actually run for one raw entry.
///
/// `None` when the config does not load, because the expanded argv is then
/// unknown - and the raw one is not a substitute: it can still hold a literal
/// `${VAULT_REF}`, so probing it would report on a command that is not the one
/// `check` runs. Positional over `config.llm`, which keeps every entry including
/// the disabled ones, so the file index indexes it directly.
fn expanded_command(
    loaded: &Result<config::Config, config::ConfigError>,
    file_index: usize,
) -> Option<&[String]> {
    loaded
        .as_ref()
        .ok()?
        .llm
        .get(file_index)?
        .api_key_command
        .as_deref()
}

/// Where this provider's key will come from, as `doctor` phrases it.
///
/// Read from the *raw* tree for the same reason the model and endpoint are: a
/// `${VAR}` shows as itself rather than being swallowed by the
/// variable-not-set error, so the report describes the file the user wrote.
///
/// The distinction is worth a line because "works on my machine" and "works in
/// CI" are different configurations, and once a stored key exists the
/// difference is invisible in `drep.toml`.
async fn key_source_line(
    entry: &Value,
    resolved_argv: Option<&[String]>,
    store: &AuthStore,
) -> String {
    let api_key = entry.get("api_key").and_then(|v| v.as_str());

    // `enabled` is passed as true because this line is only printed for entries
    // the listing has already established are in the chain.
    let source = crate::auth::source_of(
        Declared {
            api_key,
            has_api_key_command: entry.get("api_key_command").is_some(),
            endpoint: entry.get("endpoint").and_then(|v| v.as_str()),
            enabled: true,
        },
        store,
    );

    match (source, api_key) {
        // The reference is shown verbatim - that is the whole reason doctor
        // reads the raw tree rather than the loaded config.
        // Only a `${VAR}` reference is echoed. `api_key` may hold a literal
        // secret - `config::load` accepts one - and doctor's output is what
        // people paste into bug reports and CI logs.
        (KeySource::Config, Some(reference))
            if !config::env_var_refs_in(&Value::String(reference.to_string())).is_empty() =>
        {
            format!("{reference} ({})", KeySource::Config.label())
        }
        (KeySource::Config, _) => format!(
            "a literal value ({}) - prefer `${{VAR}}` so the file can be committed",
            KeySource::Config.label()
        ),
        (KeySource::Command, _) => format!(
            "{} - {}",
            KeySource::Command.label(),
            key_command_status(resolved_argv).await
        ),
        (source, _) => source.label().to_string(),
    }
}

/// Whether the configured credential helper works, and nothing else about it.
///
/// The command is really run, because this command's contract is "what will
/// actually run here" and a helper that no longer authenticates is exactly the
/// thing `api_key_command` exists to make visible. A helper behind a biometric
/// or approval prompt will therefore prompt on every `drep doctor`.
///
/// Never a byte of its output. `crate::auth::probe_key_command` returns `()`
/// rather than the credential so that is a property of the type here, not a rule
/// this function has to remember - and the failure it returns names only the
/// program and the status, for the same reason.
async fn key_command_status(argv: Option<&[String]>) -> String {
    let Some(argv) = argv else {
        return "not attempted, because the config below does not load".to_owned();
    };
    match crate::auth::probe_key_command(argv).await {
        Ok(()) => "the command ran and printed a credential".to_owned(),
        Err(err) => format!("FAILED - {err}"),
    }
}

/// Report why the config will not load, if it will not.
fn report_load_failure<W: Write>(out: &mut W, config_path: &Path) -> Result<()> {
    let loaded = config::load(config_path);
    report_load_result(out, config_path, loaded)
}

/// Shared tail of the two load-reporting paths.
fn report_load_result<W: Write>(
    out: &mut W,
    config_path: &Path,
    loaded: Result<config::Config, config::ConfigError>,
) -> Result<()> {
    if let Err(err) = loaded {
        writeln!(out, "  {} will not load: {err}", config_path.display())?;
    }
    Ok(())
}

/// Whether a raw `[[llm]]` table is in the failover chain.
///
/// The default comes from `LlmConfig::default()` rather than a literal `true`,
/// so this cannot disagree with what `config::load` will actually decide. The
/// raw table is read instead of the loaded config because `load` fails on an
/// unset `${VAR}` - and a fresh clone with no key exported is exactly when this
/// report is most useful.
fn entry_is_enabled(entry: &toml::Value) -> bool {
    entry
        .get("enabled")
        .and_then(toml::Value::as_bool)
        .unwrap_or_else(|| config::LlmConfig::default().enabled)
}

/// What the chain will actually do, given how many providers are in it.
///
/// Three genuinely different situations, and saying "providers are tried in
/// order" for a one-provider config would be true but useless - the thing that
/// user needs to know is that there is no fallback at all.
fn failover_line(enabled: usize) -> String {
    match enabled {
        0 => "Every provider is disabled - `drep check` cannot run. Re-enable one.".to_owned(),
        1 => "One provider, so there is no fallback: if it is unreachable, `drep check` exits 2."
            .to_owned(),
        n => format!(
            "{n} providers, tried in order: a transport failure falls through to the \
             next. A 401 or 403 does not - that is misconfiguration, and failing \
             over would hide it."
        ),
    }
}

/// Every variable the config references that is not set, in first-seen order.
///
/// The *reference* grammar is `config::required_env_var_refs`, shared with the
/// substituter so the two cannot disagree; all this adds is the "and it is not
/// set" filter. It excludes disabled providers for the same reason `load` does
/// not expand them: a variable only a parked provider names is not required,
/// and warning about it reports a problem `drep check` does not have.
pub(super) fn unset_env_vars(value: &toml::Value) -> Vec<String> {
    config::required_env_var_refs(value)
        .into_iter()
        .filter(|name| std::env::var_os(name).is_none())
        .collect()
}
