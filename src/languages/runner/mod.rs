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
use crate::languages::spec::{DEFAULT_TOOL_TIMEOUT_SECS, ToolSpec};

pub mod parsers;

pub use parsers::parse_output;

#[cfg(test)]
mod tests;

/// Default ceiling for deterministic tools. Individual tools can extend it
/// when their own execution model includes a legitimate wait, such as Cargo's
/// build-directory lock.
pub const TOOL_TIMEOUT: Duration = Duration::from_secs(DEFAULT_TOOL_TIMEOUT_SECS);

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
    pub compilation_succeeded: bool,
}

impl ToolOutcome {
    fn skipped(spec: &ToolSpec) -> Self {
        let listed = spec.config_files[..spec.config_files.len().min(3)].join(", ");
        Self::empty(
            spec,
            ToolStatus::Skipped,
            format!("not configured (add one of: {listed})"),
        )
    }

    fn unavailable(spec: &ToolSpec, detail: String) -> Self {
        Self::empty(spec, ToolStatus::Unavailable, detail)
    }

    fn ready(spec: &ToolSpec) -> Self {
        Self::empty(spec, ToolStatus::Ok, "ready".to_owned())
    }

    fn empty(spec: &ToolSpec, status: ToolStatus, detail: String) -> Self {
        Self {
            tool: spec.name,
            status,
            findings: Vec::new(),
            detail,
            compilation_succeeded: false,
        }
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
/// `pub(crate)` because `cli::init` needs the same answer when it decides
/// whether a git hook will actually run - git ignores a non-executable hook
/// silently, which is the same class of failure this function was written for.
/// Two definitions of "executable" that could disagree is exactly the bug.
///
/// One function with the `cfg` inside its body, not two cfg-gated
/// definitions. Two definitions means the inactive one is unreachable on
/// this platform, so every mutation of it survives by construction and
/// shows up as an untestable finding in `cargo mutants`. This way the
/// mutation lands in code the tests actually run.
pub(crate) fn is_executable(path: &Path) -> bool {
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
    resolve_tool_in(spec, root, std::env::var_os("PATH").as_deref())
}

/// `resolve_tool`, against an explicit `PATH` value. See [`which_first_in`]
/// for why this seam exists.
pub(crate) fn resolve_tool_in(
    spec: &ToolSpec,
    root: &Path,
    path: Option<&std::ffi::OsStr>,
) -> Option<PathBuf> {
    resolve_tool_at(spec, root, root, path)
}

/// Resolve a tool for a configured workspace, allowing dependencies hoisted
/// to any ancestor up to the repository root.
fn resolve_tool_at(
    spec: &ToolSpec,
    repository_root: &Path,
    workspace_root: &Path,
    path: Option<&std::ffi::OsStr>,
) -> Option<PathBuf> {
    for directory in ancestors_within(workspace_root, repository_root) {
        for relative in spec.local_paths {
            let candidate = directory.join(relative);
            if is_executable(&candidate) {
                return Some(candidate);
            }
        }
    }

    // `first()`, not `[0]`. A `ToolSpec` with an empty `command` is a
    // definitions bug, but this function is documented never to panic and a
    // panic here would take the whole gate down rather than reporting the
    // tool unavailable.
    let name = spec.command.first()?;
    which_first_in(name, path?).map(PathBuf::from)
}

pub(crate) fn absolute(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    }
}

fn ancestors_within(start: &Path, root: &Path) -> Vec<PathBuf> {
    let root = absolute(root);
    let start = absolute(start);
    if !start.starts_with(&root) {
        return Vec::new();
    }
    start
        .ancestors()
        .take_while(|ancestor| ancestor.starts_with(&root))
        .map(Path::to_path_buf)
        .collect()
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
/// `which_first`, against an explicit `PATH` value.
///
/// Split out so tests can exercise PATH lookup without mutating the process
/// environment. `std::env::set_var` is `unsafe` in edition 2024 because any
/// other thread reading the environment concurrently is a data race - and
/// these tests run beside ones that spawn `git`, which reads `PATH` to find
/// it. A test-local mutex cannot fix that: it excludes other tests that take
/// the same mutex, not every reader in the process. Passing the value in
/// removes the shared mutable state instead of guarding it.
fn which_first_in(command: &str, path: &std::ffi::OsStr) -> Option<String> {
    for dir in std::env::split_paths(path) {
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

/// The nearest configured ancestor for `file`, bounded by `repository_root`.
///
/// Looking along the file's ancestor chain avoids both monorepo blind spots
/// and accidental discovery in unrelated dependency/build directories.
pub(crate) fn configuration_root(
    spec: &ToolSpec,
    repository_root: &Path,
    file: &Path,
) -> Option<PathBuf> {
    let repository_root = absolute(repository_root);
    let file = if file.is_absolute() {
        file.to_path_buf()
    } else {
        repository_root.join(file)
    };
    ancestors_within(file.parent()?, &repository_root)
        .into_iter()
        .find(|directory| is_configured(spec, directory))
}

/// Whether this tool will run here, without running it.
///
/// The single derivation of eligibility, so `drep doctor` reports exactly
/// what `drep check` will do. Deriving it twice means doctor confidently
/// says "ready" for a tool check then skips - the failure doctor exists
/// to prevent.
pub fn tool_status(spec: &ToolSpec, root: &Path) -> ToolOutcome {
    tool_status_at(spec, root, root)
}

/// Run one deterministic tool over some files.
///
/// Never returns an error and never panics for an absent or failing tool;
/// that is reported as [`ToolStatus::Unavailable`] so the caller can surface
/// it rather than mistake it for a clean result.
pub async fn run_tool(spec: &ToolSpec, root: &Path, files: &[String]) -> ToolOutcome {
    run_tool_at(spec, root, root, files).await
}

/// Run a tool from one configured workspace, resolving hoisted executables
/// through the repository root.
pub(crate) async fn run_tool_at(
    spec: &ToolSpec,
    repository_root: &Path,
    workspace_root: &Path,
    files: &[String],
) -> ToolOutcome {
    let executable = match eligible_executable(spec, repository_root, workspace_root) {
        Ok(executable) => executable,
        Err(outcome) => return outcome,
    };

    // Absolutised before spawning. `resolve_tool` returns `root.join(relative)`
    // for a repo-local hit, and the child gets `current_dir(root)` below - so a
    // relative `root` like "repo" produced "repo/node_modules/.bin/eslint"
    // resolved *from* "repo", i.e. "repo/repo/node_modules/...". It works today
    // only because the CLI passes "." and the tests pass absolute temp dirs.
    let executable = if executable.is_relative() {
        std::env::current_dir()
            .map(|cwd| cwd.join(&executable))
            .unwrap_or(executable)
    } else {
        executable
    };

    let mut argv: Vec<String> = Vec::with_capacity(spec.command.len() + files.len());
    argv.push(executable.to_string_lossy().into_owned());
    argv.extend(spec.command[1..].iter().map(|s| (*s).to_owned()));
    // A repository can contain a file whose name begins with `-`, and every
    // checker here would read `--fix` as an option rather than a path. `--`
    // is the conventional guard but is not universally supported across
    // ruff/eslint/tsc/gofmt/go vet/clippy, whereas a `./` prefix is
    // unambiguous to any argument parser and leaves ordinary paths untouched.
    // A whole-project tool is invoked bare. `cargo clippy` rejects a path
    // argument outright ("unexpected argument"), so appending files made every
    // Rust run fail - reported honestly as `Unavailable`, which is why it
    // surfaced as exit 2 on every Rust repository rather than as wrong
    // findings. Its output is narrowed back to `files` after parsing.
    if spec.accepts_files {
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
    }

    let mut command = Command::new(&argv[0]);
    command.args(&argv[1..]);
    command.current_dir(workspace_root);
    command.stdin(Stdio::null());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());
    command.kill_on_drop(true);

    let child = match command.spawn() {
        Ok(child) => child,
        Err(err) => {
            return ToolOutcome::unavailable(
                spec,
                format!("{} could not be executed: {err}", spec.name),
            );
        }
    };

    // `wait_with_output` drains both pipes into buffers before returning,
    // and rejects on any IO error from the spawn or the read.
    let timeout = Duration::from_secs(spec.timeout_secs);
    let output = match tokio::time::timeout(timeout, child.wait_with_output()).await {
        Ok(Ok(output)) => output,
        Ok(Err(err)) => {
            return ToolOutcome::unavailable(
                spec,
                format!("{} could not be executed: {err}", spec.name),
            );
        }
        Err(_) => {
            let context = spec.timeout_context.unwrap_or_default();
            return ToolOutcome::unavailable(
                spec,
                format!(
                    "{} timed out after {}s{context}",
                    spec.name, spec.timeout_secs
                ),
            );
        }
    };

    // The exit code alone is not a verdict: ruff/eslint/clippy exit non-zero
    // *because* they found issues, and that is the success path. But it is not
    // irrelevant either. A tool that exits non-zero having produced no
    // diagnostics at all did not run - a bad config, a crash, a bad
    // invocation - and reporting that as `Ok` with zero findings is precisely
    // the "unavailable is not a pass" failure this module exists to prevent.
    // So the rule is the conjunction: non-zero AND nothing on the diagnostics
    // stream.
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
    let compilation_succeeded = spec.establishes_compilation && output.status.success();
    match parse_result {
        Ok(findings)
            if findings.is_empty() && !output.status.success() && diagnostics.trim().is_empty() =>
        {
            // Exited non-zero, said nothing on the stream we read for
            // diagnostics, and produced no findings. Whatever it did, it did
            // not check the files. The other stream usually carries the real
            // error, so it becomes the detail.
            ToolOutcome::unavailable(
                spec,
                format!(
                    "{} exited {} without producing diagnostics: {}",
                    spec.name,
                    output
                        .status
                        .code()
                        .map_or("by signal".to_owned(), |c| c.to_string()),
                    truncate(other.trim(), 200)
                ),
            )
        }
        Ok(findings) => ToolOutcome {
            tool: spec.name,
            status: ToolStatus::Ok,
            findings: retain_requested(spec, findings, files),
            detail: truncate(other.trim(), 200),
            compilation_succeeded,
        },
        Err(err) => ToolOutcome::unavailable(
            spec,
            format!("{err}. other stream: {}", truncate(other.trim(), 200)),
        ),
    }
}

pub(crate) fn tool_status_at(
    spec: &ToolSpec,
    repository_root: &Path,
    workspace_root: &Path,
) -> ToolOutcome {
    match eligible_executable(spec, repository_root, workspace_root) {
        Ok(_) => ToolOutcome::ready(spec),
        Err(outcome) => outcome,
    }
}

fn eligible_executable(
    spec: &ToolSpec,
    repository_root: &Path,
    workspace_root: &Path,
) -> Result<PathBuf, ToolOutcome> {
    if !is_configured(spec, workspace_root) {
        return Err(ToolOutcome::skipped(spec));
    }
    if let Some(executable) = resolve_tool_at(
        spec,
        repository_root,
        workspace_root,
        std::env::var_os("PATH").as_deref(),
    ) {
        return Ok(executable);
    }
    let looked = if spec.local_paths.is_empty() {
        "PATH".to_owned()
    } else if absolute(repository_root) == absolute(workspace_root) {
        format!("{}, then PATH", spec.local_paths.join(", "))
    } else {
        format!(
            "{} from {} through {}, then PATH",
            spec.local_paths.join(", "),
            workspace_root.display(),
            repository_root.display()
        )
    };
    Err(ToolOutcome::unavailable(
        spec,
        format!("configured but not found (looked in {looked})"),
    ))
}

/// Narrow a whole-project tool's findings to the files actually being checked.
///
/// A no-op for a tool that took the file list as arguments - it only reported
/// on what it was given. For one that did not (`cargo clippy`), the output
/// covers the entire crate, and a commit gate that blocked on pre-existing
/// issues in untouched code would be unusable: the author cannot fix what they
/// did not write, and every commit would fail until the whole crate was clean.
///
/// Paths are compared after stripping a leading `./`, because the tool reports
/// them relative to the project root and the caller's list may carry the
/// prefix the dash-guard adds.
fn retain_requested(spec: &ToolSpec, findings: Vec<Finding>, files: &[String]) -> Vec<Finding> {
    if spec.accepts_files {
        return findings;
    }
    let wanted: std::collections::BTreeSet<&str> =
        files.iter().map(|f| normalize_path(f)).collect();
    findings
        .into_iter()
        .filter(|finding| wanted.contains(normalize_path(&finding.file_path)))
        .collect()
}

/// A path with any leading `./` removed, for comparison.
fn normalize_path(path: &str) -> &str {
    path.strip_prefix("./").unwrap_or(path)
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
