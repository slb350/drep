//! `drep doctor` - report what drep can actually do in this repository.
//!
//! Adoption question, not a debugging one. Before trusting drep as a gate, a
//! user wants to know the real coverage here: which languages are present,
//! which of their own tools will actually run, and whether the LLM half is
//! configured.
//!
//! **Diagnostic findings never fail `doctor`.** A broken provider or missing
//! tool still returns `Ok(Exit::Clean)`; it is diagnosis, and `drep check` is
//! the gate. Ordinary I/O failures can still be returned when the report
//! itself cannot be written or the platform cannot resolve its user paths.
//!
//! All output goes through a `&mut dyn std::io::Write` so the command is
//! testable without spawning a subprocess. The tests call [`run_to`] directly
//! against a captured buffer.

use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::Result;
use clap::Args;

use crate::Exit;
use crate::config;
use crate::files;
use crate::languages;
use toml::Value;

/// The header underline, exactly 60 characters wide. `write!` cannot express
/// the count cleanly, and the spec pins the exact width: a `=`-string of any
/// other length fails A2.
const HEADER_RULE: &str = "============================================================";

#[derive(Debug, Args)]
pub struct DoctorArgs {
    /// Repository or directory to report on.
    #[arg(value_name = "PATH", default_value = ".")]
    pub path: PathBuf,

    /// Config file to report on. Defaults to `drep.toml` under PATH.
    #[arg(long, value_name = "FILE")]
    pub config: Option<PathBuf>,
}

/// Run the command, writing to stdout. Diagnostic findings return
/// `Ok(Exit::Clean)`; failures to produce the report remain ordinary errors.
pub fn run(args: &DoctorArgs) -> Result<Exit> {
    let mut out = std::io::stdout().lock();
    match run_to(&mut out, args) {
        Ok(exit) => Ok(exit),
        // `drep doctor | head -5` closes the pipe under us. That is the
        // reader's choice, not a diagnostic failure, and turning it into exit 2
        // would contradict this command's one contract.
        Err(err) if is_broken_pipe(&err) => Ok(Exit::Clean),
        Err(err) => Err(err),
    }
}

/// Whether `err` is the reader having closed the pipe.
pub(crate) fn is_broken_pipe(err: &anyhow::Error) -> bool {
    err.downcast_ref::<std::io::Error>()
        .is_some_and(|io| io.kind() == std::io::ErrorKind::BrokenPipe)
}

/// `run`, writing to an arbitrary sink so tests can capture the report.
pub fn run_to<W: Write>(out: &mut W, args: &DoctorArgs) -> Result<Exit> {
    run_at(out, args, &crate::auth::default_path()?)
}

/// `run_to`, against a named auth store.
///
/// A parameter for the same reason `check`, `init` and `auth` take one: the
/// store is user-level state, and a test reading the real one reports whatever
/// the developer happens to have stored.
pub fn run_at<W: Write>(out: &mut W, args: &DoctorArgs, auth_path: &Path) -> Result<Exit> {
    run_at_with_codex(out, args, auth_path, &crate::llm::codex::current_status)
}

/// [`run_at`] with the Codex readiness diagnostic injected for tests.
pub(crate) fn run_at_with_codex<W: Write>(
    out: &mut W,
    args: &DoctorArgs,
    auth_path: &Path,
    codex_probe: &dyn Fn() -> Result<crate::llm::codex::CodexStatus, String>,
) -> Result<Exit> {
    // `canonicalize` can fail (the path does not exist, or a parent is
    // unreadable). An unreadable path is still worth reporting on - the user
    // has typed something and wants to know what drep sees - so fall back to
    // the path as given rather than erroring out.
    let root = args
        .path
        .canonicalize()
        .unwrap_or_else(|_| args.path.clone());

    writeln!(out, "drep in {}", root.display())?;
    writeln!(out, "{HEADER_RULE}")?;

    let files = files::expand_paths(std::slice::from_ref(&root), files::is_scan_target);
    let file_refs: Vec<&Path> = files.iter().map(PathBuf::as_path).collect();
    let buckets = languages::group_by_language(&file_refs);

    if buckets.is_empty() {
        writeln!(out)?;
        writeln!(out, "No source files drep recognises were found here.")?;
        // The LLM section still prints. "Is my model configured?" is the
        // question a new user most needs answered, and a docs-only repo - or
        // one whose languages drep does not register - is exactly where they
        // are most likely to be asking it. Returning here answered it with
        // silence.
        write_llm_section(out, args, &root, auth_path, codex_probe)?;
        return Ok(Exit::Clean);
    }

    write_languages_section(out, &buckets)?;
    // The missing list falls out of the same pass that printed the tool
    // lines. Recomputing it afterwards meant asking `tool_status` twice per
    // tool - each call stats the config files and walks PATH - and, worse,
    // left room for the summary to disagree with the lines above it.
    let missing = write_tools_section(out, &buckets, &root)?;
    write_llm_section(out, args, &root, auth_path, codex_probe)?;

    // Deliberately last, after the LLM block: the user reads their coverage
    // report before being told what is wrong with it.
    if let Some(line) = missing_tools_line(&missing) {
        writeln!(out)?;
        writeln!(out, "{line}")?;
    }

    Ok(Exit::Clean)
}

/// Build the trailing "configured tool(s) are missing" line, or `None` when
/// there is nothing to report.
///
/// Extracted so A4 can pin the rendering independently of the runner's
/// availability on a particular developer machine.
fn missing_tools_line(missing: &[&str]) -> Option<String> {
    if missing.is_empty() {
        return None;
    }
    Some(format!(
        "{} configured tool(s) are missing: {}. drep exits 2 rather than reporting those files clean.",
        missing.len(),
        missing.join(", "),
    ))
}

/// `Languages found:` block, one line per detected language.
fn write_languages_section<W: Write>(
    out: &mut W,
    buckets: &[(&'static languages::spec::LanguageSupport, Vec<&Path>)],
) -> Result<()> {
    writeln!(out)?;
    writeln!(out, "Languages found:")?;
    for (language, paths) in buckets {
        writeln!(out, "  {}: {} file(s)", language.display_name, paths.len())?;
    }
    Ok(())
}

/// `Deterministic checks (these gate):` block. Tool status comes from
/// `runner::tool_status` so `doctor` cannot disagree with `check` about
/// whether a tool will run.
///
/// Returns the names of the tools that were `Unavailable`, so the trailing
/// summary is built from the same statuses that were printed rather than from
/// a second round of `tool_status` calls.
fn write_tools_section<W: Write>(
    out: &mut W,
    buckets: &[(&'static languages::spec::LanguageSupport, Vec<&Path>)],
    root: &Path,
) -> Result<Vec<&'static str>> {
    writeln!(out)?;
    writeln!(out, "Deterministic checks (these gate):")?;
    let mut missing: Vec<&'static str> = Vec::new();
    for (language, paths) in buckets {
        if language.tools.is_empty() {
            writeln!(out, "  {}: no tools wired up yet", language.display_name)?;
            continue;
        }
        for spec in language.tools {
            let roots: BTreeSet<PathBuf> = paths
                .iter()
                .filter_map(|path| languages::runner::configuration_root(spec, root, path))
                .collect();
            let outcome = if roots.is_empty() {
                languages::runner::tool_status(spec, root)
            } else {
                workspace_tool_status(spec, root, &roots)
            };
            writeln!(out, "  {}: {}", spec.name, outcome.detail)?;
            // `Skipped` is the project exercising a choice, not a problem.
            // Rendering it as one trains users to ignore the report.
            if matches!(outcome.status, languages::runner::ToolStatus::Unavailable)
                && !missing.contains(&spec.name)
            {
                // Deduplicated: `eslint` belongs to both JavaScript and
                // TypeScript, so a repo with both and no eslint binary
                // otherwise reported "2 configured tool(s) are missing:
                // eslint, eslint" - a count that overstates the problem and a
                // list that reads like a bug.
                missing.push(spec.name);
            }
        }
    }
    Ok(missing)
}

fn workspace_tool_status(
    spec: &'static languages::spec::ToolSpec,
    root: &Path,
    roots: &BTreeSet<PathBuf>,
) -> languages::runner::ToolOutcome {
    let statuses: Vec<_> = roots
        .iter()
        .map(|workspace| languages::runner::tool_status_at(spec, root, workspace))
        .collect();
    if let Some(unavailable) = statuses
        .iter()
        .find(|outcome| matches!(outcome.status, languages::runner::ToolStatus::Unavailable))
    {
        return unavailable.clone();
    }
    let detail = if roots.len() == 1 && roots.contains(&root.to_path_buf()) {
        "ready".to_owned()
    } else {
        format!("ready in {} workspace(s)", roots.len())
    };
    languages::runner::ToolOutcome {
        tool: spec.name,
        status: languages::runner::ToolStatus::Ok,
        findings: Vec::new(),
        detail,
        compilation_succeeded: false,
    }
}

/// `LLM analysis (required):` block.
///
/// Display path is the *raw* file, not `config::load`: a fresh clone is
/// exactly when the report is most useful, and `load` fails on an unset
/// referenced variable. `load` is consulted only to surface problems that are
/// not the unset variable.
fn write_llm_section<W: Write>(
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
        .then(|| crate::auth::AuthStore::load(auth_path))
        .transpose()
    {
        Ok(Some(store)) => store,
        Ok(None) => crate::auth::AuthStore::new(),
        Err(err) => {
            writeln!(out, "  The auth store could not be read: {err}")?;
            crate::auth::AuthStore::new()
        }
    };

    let mut enabled_count = 0usize;
    let mut codex_status: Option<Result<crate::llm::codex::CodexStatus, String>> = None;
    for entry in providers {
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
                writeln!(out, "     key: {}", key_source_line(entry, &store))?;
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
    match config::load(&config_path) {
        Err(config::ConfigError::EnvVarUnset(_, _)) => Ok(()),
        other => report_load_result(out, &config_path, other),
    }
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
fn key_source_line(entry: &Value, store: &crate::auth::AuthStore) -> String {
    let api_key = entry.get("api_key").and_then(|v| v.as_str());
    let endpoint = entry.get("endpoint").and_then(|v| v.as_str());

    // `enabled` is passed as true because this line is only printed for entries
    // the listing has already established are in the chain.
    let source = crate::auth::source_of(api_key, endpoint, true, store);

    match (source, api_key) {
        // The reference is shown verbatim - that is the whole reason doctor
        // reads the raw tree rather than the loaded config.
        // Only a `${VAR}` reference is echoed. `api_key` may hold a literal
        // secret - `config::load` accepts one - and doctor's output is what
        // people paste into bug reports and CI logs.
        (crate::auth::KeySource::Config, Some(reference))
            if !crate::config::env_var_refs_in(&Value::String(reference.to_string()))
                .is_empty() =>
        {
            format!("{reference} ({})", crate::auth::KeySource::Config.label())
        }
        (crate::auth::KeySource::Config, _) => format!(
            "a literal value ({}) - prefer `${{VAR}}` so the file can be committed",
            crate::auth::KeySource::Config.label()
        ),
        (source, _) => source.label().to_string(),
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
fn unset_env_vars(value: &toml::Value) -> Vec<String> {
    config::required_env_var_refs(value)
        .into_iter()
        .filter(|name| std::env::var_os(name).is_none())
        .collect()
}

#[cfg(test)]
mod unit_tests;

/// Acceptance tests live in their own directory under `tests/`, declared
/// from this module. The directory has its own `mod.rs` so the files there
/// are reachable by name - a Rust file no `mod` declaration reaches is never
/// compiled, and a test file that is never compiled looks exactly like a
/// passing one.
#[cfg(test)]
mod tests;
