//! `go list`, asked which requested files the build includes and which packages fail to load, before `go vet` runs over their packages. Why it runs first is on [`ToolArguments::GoPackages`](crate::languages::spec::ToolArguments::GoPackages).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use tokio::time::Instant;

use super::{Bounded, TYPE_CHECK_KIND, exit_word, run_bounded, stream_detail};
use crate::analysis::findings::{Finding, Severity};

/// What `go list` said about the requested files.
#[derive(Debug, Default, PartialEq)]
pub(super) struct Plan {
    /// `./dir` for each package that loads and holds a requested file the build includes, each once.
    pub(super) directories: Vec<String>,
    /// Requested files the build excludes, as given.
    pub(super) excluded: Vec<String>,
    /// The load errors of packages that hold a requested file the build includes, as [`TYPE_CHECK_KIND`] findings.
    pub(super) load_errors: Vec<Finding>,
}

/// The `go list -json` fields drep reads. Every file list holds bare names within `dir`.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
struct Package {
    /// Absent when `go` could not place the directory in the workspace, a module `go.work` does not use say.
    dir: Option<PathBuf>,
    go_files: Vec<String>,
    cgo_files: Vec<String>,
    test_go_files: Vec<String>,
    x_test_go_files: Vec<String>,
    /// Files in the package whose build constraints do not parse. They are not excluded: `go vet` rejects them, and `error` says so.
    invalid_go_files: Vec<String>,
    error: Option<PackageError>,
    deps_errors: Vec<PackageError>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct PackageError {
    #[serde(default)]
    pos: String,
    err: String,
}

const LIST_FIELDS: &str =
    "-json=Dir,GoFiles,CgoFiles,TestGoFiles,XTestGoFiles,InvalidGoFiles,Error,DepsErrors";

/// List the package of every one of `files` (workspace-relative) with `go`, from `workspace_root`.
///
/// An error is the detail for an `Unavailable` outcome: `go list` failing, timing out, printing what is not its JSON, or leaving a requested directory unreported means drep does not know what vet would cover.
pub(super) async fn plan(
    go: &Path,
    workspace_root: &Path,
    files: &[String],
    deadline: Instant,
) -> Result<Plan, String> {
    let mut by_directory: BTreeMap<String, Vec<&String>> = BTreeMap::new();
    for file in files {
        by_directory
            .entry(directory_argument(Path::new(file)))
            .or_default()
            .push(file);
    }
    let packages = list(go, workspace_root, by_directory.keys(), deadline).await?;
    let mut by_dir: HashMap<PathBuf, Package> = HashMap::new();
    for package in packages {
        let Some(dir) = package.dir.as_deref().map(canonical) else {
            // go writes the directory's name into this message unquoted, and the name is repository content: a raw escape sequence or a line break once it is decoded from JSON.
            let reason = package
                .error
                .as_ref()
                .map_or_else(|| "no reason given".to_owned(), |e| stream_detail(&e.err));
            return Err(format!(
                "go list could not place a requested directory: {reason}"
            ));
        };
        by_dir.insert(dir, package);
    }

    let mut plan = Plan::default();
    for (directory, requested) in by_directory {
        let Some(package) = by_dir.get(&canonical(&workspace_root.join(&directory))) else {
            return Err(format!("go list did not report {directory}"));
        };
        let members: BTreeSet<&str> = [
            &package.go_files,
            &package.cgo_files,
            &package.test_go_files,
            &package.x_test_go_files,
            &package.invalid_go_files,
        ]
        .into_iter()
        .flatten()
        .map(String::as_str)
        .collect();
        let in_build = |file: &str| {
            Path::new(file)
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| members.contains(name))
        };
        let (built, excluded): (Vec<&String>, Vec<&String>) =
            requested.into_iter().partition(|file| in_build(file));
        plan.excluded.extend(excluded.into_iter().cloned());
        let Some(first) = built.first() else {
            continue;
        };
        if package.error.is_none() && package.deps_errors.is_empty() {
            plan.directories.push(directory);
        } else {
            let errors = package.error.iter().chain(&package.deps_errors);
            plan.load_errors
                .extend(errors.map(|error| load_error(error, first)));
        }
    }
    Ok(plan)
}

/// The directory `file` sits in, behind `./` so `go` reads it as a path rather than an import path, or `.` at the workspace root. `file` is workspace-relative, as `plan_tasks` builds every argument.
fn directory_argument(file: &Path) -> String {
    match file.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => {
            format!("./{}", parent.to_string_lossy())
        }
        _ => ".".to_owned(),
    }
}

/// `go` reports `Dir` through resolved symlinks (`/private/var` for a macOS temp directory), so both sides are compared canonical.
fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

async fn list(
    go: &Path,
    workspace_root: &Path,
    directories: impl Iterator<Item = &String>,
    deadline: Instant,
) -> Result<Vec<Package>, String> {
    let args = ["list", "-e", LIST_FIELDS]
        .iter()
        .map(|arg| arg.to_string())
        .chain(directories.cloned());
    let output = match run_bounded(go, args, workspace_root, deadline).await {
        Ok(output) => output,
        Err(Bounded::Failed(err)) => return Err(format!("go list could not be executed: {err}")),
        Err(Bounded::TimedOut) => return Err("go list timed out".to_owned()),
    };
    // `-e` turns a package's own errors into its `Error` field and exits 0, so a non-zero exit means `go` itself could not run: a broken `go.mod`, an unknown flag.
    if !output.status.success() {
        return Err(format!(
            "go list exited {}: {}",
            exit_word(&output.status),
            stream_detail(&String::from_utf8_lossy(&output.stderr))
        ));
    }
    serde_json::Deserializer::from_slice(&output.stdout)
        .into_iter::<Package>()
        .collect::<Result<_, _>>()
        .map_err(|err| format!("go list printed what drep could not read: {err}"))
}

/// One load error as a finding at its own position, which `go` writes as `file:line:col` relative to the workspace. An error with no position is placed on `fallback`, a requested file of the package, so it cannot go unreported.
fn load_error(error: &PackageError, fallback: &str) -> Finding {
    let (file, line, column) = position(&error.pos).unwrap_or((fallback, 1, None));
    Finding::deterministic(
        TYPE_CHECK_KIND.to_owned(),
        Severity::Error,
        file.strip_prefix("./").unwrap_or(file).to_owned(),
        line,
        column,
        error.err.clone(),
        None,
    )
}

/// `file:line:col`, `file:line`, or a bare `file`, split from the right so a Windows drive letter stays in the file. `None` for an empty position.
pub(super) fn position(pos: &str) -> Option<(&str, u32, Option<u32>)> {
    if pos.is_empty() {
        return None;
    }
    let Some((head, last)) = pos
        .rsplit_once(':')
        .and_then(|(head, last)| Some((head, last.parse::<u32>().ok()?)))
    else {
        return Some((pos, 1, None));
    };
    match head
        .rsplit_once(':')
        .and_then(|(file, line)| Some((file, line.parse::<u32>().ok()?)))
    {
        Some((file, line)) => Some((file, line, Some(last))),
        None => Some((head, last, None)),
    }
}
