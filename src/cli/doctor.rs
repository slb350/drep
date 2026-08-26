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
use crate::files;
use crate::languages;

mod llm;
mod site_section;

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
pub async fn run(args: &DoctorArgs) -> Result<Exit> {
    let mut out = std::io::stdout().lock();
    match run_to(&mut out, args).await {
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
pub async fn run_to<W: Write>(out: &mut W, args: &DoctorArgs) -> Result<Exit> {
    run_at(
        out,
        args,
        &crate::auth::default_path()?,
        &crate::config::site::default_path(),
    )
    .await
}

/// `run_to`, against a named auth store and a named site policy file.
///
/// Both are parameters for the same reason `check`, `init` and `auth` take the
/// store: they are machine-level state, and a test reading the real ones reports
/// whatever the developer happens to have installed.
pub async fn run_at<W: Write>(
    out: &mut W,
    args: &DoctorArgs,
    auth_path: &Path,
    site_path: &Path,
) -> Result<Exit> {
    run_at_with_codex(
        out,
        args,
        auth_path,
        site_path,
        &crate::llm::codex::current_status,
    )
    .await
}

/// [`run_at`] with the Codex readiness diagnostic injected for tests.
pub(crate) async fn run_at_with_codex<W: Write>(
    out: &mut W,
    args: &DoctorArgs,
    auth_path: &Path,
    site_path: &Path,
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
        // The configuration sections still print. "Is my model configured?" is
        // the question a new user most needs answered, and a docs-only repo - or
        // one whose languages drep does not register - is exactly where they
        // are most likely to be asking it. Returning here answered it with
        // silence.
        write_configuration(out, args, &root, auth_path, site_path, codex_probe).await?;
        return Ok(Exit::Clean);
    }

    write_languages_section(out, &buckets)?;
    // The missing list falls out of the same pass that printed the tool
    // lines. Recomputing it afterwards meant asking `tool_status` twice per
    // tool - each call stats the config files and walks PATH - and, worse,
    // left room for the summary to disagree with the lines above it.
    let missing = write_tools_section(out, &buckets, &root)?;
    write_configuration(out, args, &root, auth_path, site_path, codex_probe).await?;

    // Deliberately last, after the LLM block: the user reads their coverage
    // report before being told what is wrong with it.
    if let Some(line) = missing_tools_line(&missing) {
        writeln!(out)?;
        writeln!(out, "{line}")?;
    }

    Ok(Exit::Clean)
}

/// The two configuration blocks, in the one order they are ever printed in.
///
/// Called from both report shapes so that order is stated once. The policy block
/// comes first because it governs the chain the block below it describes, and the
/// policy file is loaded once here rather than in each block: two loads of the
/// same file could disagree about it within one report.
async fn write_configuration<W: Write>(
    out: &mut W,
    args: &DoctorArgs,
    root: &Path,
    auth_path: &Path,
    site_path: &Path,
    codex_probe: &dyn Fn() -> Result<crate::llm::codex::CodexStatus, String>,
) -> Result<()> {
    let site = crate::config::site::load(site_path);
    site_section::write_site_section(out, site_path, &site)?;
    let in_effect = site.as_ref().ok().and_then(Option::as_ref);
    llm::write_llm_section(out, args, root, auth_path, in_effect, codex_probe).await
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

#[cfg(test)]
mod unit_tests;

/// Acceptance tests live in their own directory under `tests/`, declared
/// from this module. The directory has its own `mod.rs` so the files there
/// are reachable by name - a Rust file no `mod` declaration reaches is never
/// compiled, and a test file that is never compiled looks exactly like a
/// passing one.
#[cfg(test)]
mod tests;
