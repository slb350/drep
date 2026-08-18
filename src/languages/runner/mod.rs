//! Run a project's own deterministic checkers and turn their output into Findings.
//!
//! This is the gating half of analysis. Tool findings are precise - they come
//! from the rules the project itself configured - so they block, while the
//! LLM's semantic findings inform. Keeping the two apart by *source* rather
//! than by severity is what makes the gate calibratable at all.
//!
//! Three states, deliberately distinct:
//!
//! - [`ToolStatus::Ok`] the tool ran; its findings are authoritative.
//! - [`ToolStatus::Skipped`] the project has not configured this tool, so it
//!   has no opinion here. A pass.
//! - [`ToolStatus::Unavailable`] the tool should have run and could not.
//!   **Not** a pass - reporting it as clean is the same "unanalyzed is not
//!   clean" mistake that would let a commit gate rubber-stamp commits.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use tokio::process::Command;

use crate::analysis::findings::Finding;
use crate::languages::spec::ToolSpec;

pub mod parsers;

pub use parsers::parse_output;

#[cfg(test)]
mod tests;

/// A tool that has not produced output by now is hung, not slow.
///
/// Deterministic checkers are fast; this is not the LLM path. The Python
/// reference uses 120s; matching it keeps the two implementations comparable
/// in bug reports.
pub const TOOL_TIMEOUT: Duration = Duration::from_secs(120);

/// Whether a tool ran, declined to run, or failed to.
///
/// The three are distinct because only the third is a problem: `Skipped` is
/// the project exercising a choice, `Unavailable` is drep failing to check
/// something it was supposed to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolStatus {
    /// The tool ran. Its findings are authoritative.
    Ok,
    /// The project has not configured this tool, so it has no opinion here.
    Skipped,
    /// The tool should have run and could not. **Not** a pass.
    Unavailable,
}

/// What happened when one tool was asked to check some files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolOutcome {
    pub tool: &'static str,
    pub status: ToolStatus,
    pub findings: Vec<Finding>,
    pub detail: String,
}

impl ToolOutcome {
    /// Whether this outcome is safe to treat as "nothing wrong here".
    ///
    /// `Unavailable` is not: the check never happened. The Rust type makes
    /// that impossible to forget - `passed()` only returns `true` for `Ok`
    /// and `Skipped`, both of which genuinely mean the tool got to look at
    /// the code (or deliberately chose not to).
    pub const fn passed(&self) -> bool {
        !matches!(self.status, ToolStatus::Unavailable)
    }
}

/// The tool produced output we could not parse.
///
/// Raised rather than swallowed: unparseable output means we do not know
/// whether the file is clean, and guessing "clean" is the failure this module
/// exists to prevent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolOutputError(pub String);

impl std::fmt::Display for ToolOutputError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ToolOutputError {}

/// Whether a path is a regular file that the OS will execute.
///
/// One function with the `cfg` inside its body, not two cfg-gated
/// definitions. Two definitions means the inactive one is unreachable on
/// this platform, so every mutation of it survives by construction and
/// shows up as an untestable finding in `cargo mutants`. This way the
/// mutation lands in code the tests actually run.
fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        match path.metadata() {
            Ok(meta) => meta.is_file() && meta.permissions().mode() & 0o111 != 0,
            Err(_) => false,
        }
    }
    // Windows has no executable bit; existence is the only signal there is.
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

/// Find the executable for a tool, preferring the project's own copy.
///
/// Repo-local first so a project is checked by the version its CI runs -
/// `node_modules/.bin/eslint` rather than whatever happens to be installed
/// globally, which may resolve plugins differently or not at all.
///
/// A path that exists but is not executable is **skipped**, not failed -
/// half-installed shims on `PATH` would otherwise block a tool that
/// happens to be on PATH under the same name.
pub fn resolve_tool(spec: &ToolSpec, root: &Path) -> Option<PathBuf> {
    for relative in spec.local_paths {
        let candidate = root.join(relative);
        if is_executable(&candidate) {
            return Some(candidate);
        }
    }

    // `first()`, not `[0]`. A `ToolSpec` with an empty `command` is a
    // definitions bug, but this function is documented never to panic and a
    // panic here would take the whole gate down rather than reporting the
    // tool unavailable.
    let name = spec.command.first()?;
    which_first(name).map(PathBuf::from)
}

/// Look up `command` on PATH, mirroring `shutil.which` from the Python
/// reference (which `std::process::Command` does not expose directly).
///
/// Crucially checks executability, not just existence: a half-installed
/// shim on PATH that is not executable would otherwise be reported as `Ok`
/// by `tool_status` only to fail when `run_tool` actually executed it.
/// Using the same `is_executable` helper the repo-local branch uses keeps
/// the two paths consistent — a path that passes one is rejected by the
/// other is the bug this guard exists to prevent.
fn which_first(command: &str) -> Option<String> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(command);
        if is_executable(&candidate) {
            return Some(candidate.to_string_lossy().into_owned());
        }
    }
    None
}

/// Whether the project has opted into this tool.
///
/// "Style adherence where defined": a repo with no eslint config has not
/// chosen eslint's defaults, so running it would invent findings the
/// project never asked for.
pub fn is_configured(spec: &ToolSpec, root: &Path) -> bool {
    spec.config_files
        .iter()
        .any(|name| root.join(name).exists())
}

/// Whether this tool will run here, without running it.
///
/// The single derivation of eligibility, so `drep doctor` reports exactly
/// what `drep check` will do. Deriving it twice means doctor confidently
/// says "ready" for a tool check then skips - the failure doctor exists
/// to prevent.
pub fn tool_status(spec: &ToolSpec, root: &Path) -> ToolOutcome {
    if !is_configured(spec, root) {
        let listed = spec.config_files[..spec.config_files.len().min(3)].join(", ");
        return ToolOutcome {
            tool: spec.name,
            status: ToolStatus::Skipped,
            findings: Vec::new(),
            detail: format!("not configured (add one of: {listed})"),
        };
    }

    if resolve_tool(spec, root).is_none() {
        let looked = if spec.local_paths.is_empty() {
            "PATH".to_owned()
        } else {
            format!("{}, then PATH", spec.local_paths.join(", "))
        };
        return ToolOutcome {
            tool: spec.name,
            status: ToolStatus::Unavailable,
            findings: Vec::new(),
            detail: format!("configured but not found (looked in {looked})"),
        };
    }

    ToolOutcome {
        tool: spec.name,
        status: ToolStatus::Ok,
        findings: Vec::new(),
        detail: "ready".to_owned(),
    }
}

/// Run one deterministic tool over some files.
///
/// Never returns an error and never panics for an absent or failing tool;
/// that is reported as [`ToolStatus::Unavailable`] so the caller can surface
/// it rather than mistake it for a clean result.
pub async fn run_tool(spec: &ToolSpec, root: &Path, files: &[String]) -> ToolOutcome {
    let eligibility = tool_status(spec, root);
    if eligibility.status != ToolStatus::Ok {
        return eligibility;
    }

    // `tool_status` just confirmed the resolution succeeded, but the type
    // system needs the unwrap; the `if` above is the real check.
    let Some(executable) = resolve_tool(spec, root) else {
        return ToolOutcome {
            tool: spec.name,
            status: ToolStatus::Unavailable,
            findings: Vec::new(),
            detail: "configured but not found".to_owned(),
        };
    };

    let mut argv: Vec<String> = Vec::with_capacity(spec.command.len() + files.len());
    argv.push(executable.to_string_lossy().into_owned());
    argv.extend(spec.command[1..].iter().map(|s| (*s).to_owned()));
    // A repository can contain a file whose name begins with `-`, and every
    // checker here would read `--fix` as an option rather than a path. `--`
    // is the conventional guard but is not universally supported across
    // ruff/eslint/tsc/gofmt/go vet/clippy, whereas a `./` prefix is
    // unambiguous to any argument parser and leaves ordinary paths untouched.
    argv.extend(files.iter().map(|f| {
        if f.starts_with('-') {
            format!("./{f}")
        } else {
            f.clone()
        }
    }));

    let mut command = Command::new(&argv[0]);
    command.args(&argv[1..]);
    command.current_dir(root);
    command.stdin(Stdio::null());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());
    command.kill_on_drop(true);

    let child = match command.spawn() {
        Ok(child) => child,
        Err(err) => {
            return ToolOutcome {
                tool: spec.name,
                status: ToolStatus::Unavailable,
                findings: Vec::new(),
                detail: format!("{} could not be executed: {err}", spec.name),
            };
        }
    };

    // `wait_with_output` drains both pipes into buffers before returning,
    // and rejects on any IO error from the spawn or the read.
    let output = match tokio::time::timeout(TOOL_TIMEOUT, child.wait_with_output()).await {
        Ok(Ok(output)) => output,
        Ok(Err(err)) => {
            return ToolOutcome {
                tool: spec.name,
                status: ToolStatus::Unavailable,
                findings: Vec::new(),
                detail: format!("{} could not be executed: {err}", spec.name),
            };
        }
        Err(_) => {
            return ToolOutcome {
                tool: spec.name,
                status: ToolStatus::Unavailable,
                findings: Vec::new(),
                detail: format!("{} timed out after {}s", spec.name, TOOL_TIMEOUT.as_secs()),
            };
        }
    };

    // The exit code is irrelevant: ruff/eslint/clippy exit non-zero when
    // they find issues, and that is the success path. The only question is
    // whether the diagnostics stream parsed.
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let (diagnostics, other) = if spec.diagnostics_stream == "stderr" {
        (stderr.as_ref(), stdout.as_ref())
    } else {
        (stdout.as_ref(), stderr.as_ref())
    };

    let parse_result = parse_output(
        spec,
        diagnostics,
        files.first().map(String::as_str).unwrap_or(""),
    );
    match parse_result {
        Ok(findings) => ToolOutcome {
            tool: spec.name,
            status: ToolStatus::Ok,
            findings,
            detail: truncate(other.trim(), 200),
        },
        Err(err) => ToolOutcome {
            tool: spec.name,
            status: ToolStatus::Unavailable,
            findings: Vec::new(),
            detail: format!("{err}. other stream: {}", truncate(other.trim(), 200)),
        },
    }
}

/// Truncate a string to at most `max` bytes, on a char boundary.
///
/// The Python reference uses `s.strip()[:200]`, which slices bytes. For
/// ASCII that is identical; for multibyte UTF-8 we still need to land on
/// a boundary to keep the result valid.
pub(crate) fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_owned();
    }
    // Searched rather than walked with a mutable counter. A `while` loop
    // decrementing `end` is correct, but it admits a mutation - `-=` swapped for
    // `/=`, which is `end / 1` and therefore a no-op - that spins forever
    // instead of failing. An infinite loop is a worse failure mode than a wrong
    // answer, and this formulation cannot express it: the range is finite.
    //
    // Byte 0 is always a char boundary, so the search always succeeds.
    let end = (0..=max)
        .rev()
        .find(|&i| s.is_char_boundary(i))
        .unwrap_or(0);
    s[..end].to_owned()
}
