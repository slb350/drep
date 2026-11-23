//! Narrow a whole-project or Go package tool's findings to the files drep was asked to check.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::{TYPE_CHECK_KIND, lexically_normal};
use crate::analysis::findings::Finding;
use crate::languages::spec::{ToolArguments, ToolSpec};

/// Narrow a tool's findings to the files actually being checked.
///
/// A whole-project tool (`cargo clippy`, `tsc`, `dotnet format`) reports on
/// everything it compiled, and a commit gate that blocked on pre-existing
/// issues in untouched code would be unusable: the author cannot fix what they
/// did not write, and every commit would fail until the whole project was
/// clean.
///
/// A Go package tool (`go vet`) is narrowed the same way: handed `./pkg`, it reports on every file in the package, test files included. A [`TYPE_CHECK_KIND`] finding is kept wherever it points; see that constant for why. So is a finding outside every directory it was handed: `go vet` analyzes only those, so it names a dependency that did not compile, and the package that imports it was not vetted.
///
/// A no-op for a tool that took the file list as arguments - it only reported
/// on what it was given. cppcheck is the first shipped tool where that is not
/// quite true, since it follows `#include` and can raise a finding in a header
/// the commit never touched; that is left alone here deliberately, because
/// narrowing every tool changes a contract this module documents and tests
/// rather than fixing a defect.
///
/// Comparison is on absolute paths, in both forms the two sides can spell
/// them. Stripping a leading `./` is not enough: the caller's list is
/// workspace-relative (`plan_tasks` builds each argument by
/// `strip_prefix(workspace_root)`) while a tool is free to answer in either
/// form, and `dotnet format` answers with the absolute path on every line.
/// And the tool's absolute form can be *canonical* where drep's is not: the
/// child runs with `current_dir(workspace_root)`, so a tool deriving paths
/// from its cwd spells them through resolved symlinks (and emits
/// `../..`-carrying relatives, as tflint `--recursive` does under one) while
/// drep keeps the spelling it was given. Byte-exact against one form, those
/// never match, the filter empties the vector and every file is reported
/// clean - the silent pass this module exists to refuse. `Path::join` with
/// an absolute argument yields that argument, so one join normalises both
/// forms.
///
/// The canonical set is built only once a finding has actually missed on the
/// exact form. Under an ordinary checkout every finding matches exactly, and
/// resolving each requested file up front spends a `realpath` per file to
/// answer a question nothing asks.
pub(super) fn retain_requested(
    spec: &ToolSpec,
    findings: Vec<Finding>,
    files: &[String],
    directories: &[String],
    workspace_root: &Path,
) -> Vec<Finding> {
    if spec.arguments == ToolArguments::Files || findings.is_empty() {
        return findings;
    }
    let wanted: HashSet<PathBuf> = files
        .iter()
        .map(|file| joined_reported(workspace_root, file))
        .collect();
    let mut wanted_canonical = LazyCanonical::of(&wanted);
    // `go vet` analyzes only the directories it was handed, so a finding anywhere else came from a dependency that did not compile. A directory whose only requested file the build excludes was not handed.
    let go_packages = spec.arguments == ToolArguments::GoPackages;
    let vetted: HashSet<PathBuf> = if go_packages {
        directories
            .iter()
            .map(|directory| lexically_normal(&workspace_root.join(directory)))
            .collect()
    } else {
        HashSet::new()
    };
    let mut vetted_canonical = LazyCanonical::of(&vetted);
    findings
        .into_iter()
        .filter(|finding| {
            if finding.kind == TYPE_CHECK_KIND {
                return true;
            }
            let joined = joined_reported(workspace_root, &finding.file_path);
            if wanted.contains(&joined) || wanted_canonical.contains(joined.canonicalize().ok()) {
                return true;
            }
            if !go_packages {
                return false;
            }
            let directory = joined.parent();
            let in_vetted_directory = directory.is_some_and(|directory| vetted.contains(directory))
                || vetted_canonical.contains(directory.and_then(|d| d.canonicalize().ok()));
            !in_vetted_directory
        })
        .collect()
}

/// The canonical forms of a set of paths, resolved on first use.
///
/// Every member costs a `realpath`, so the set is worth building only once
/// something has failed to match in exact form - which under an ordinary
/// checkout is never.
struct LazyCanonical<'a> {
    paths: &'a HashSet<PathBuf>,
    resolved: Option<HashSet<PathBuf>>,
}

impl<'a> LazyCanonical<'a> {
    fn of(paths: &'a HashSet<PathBuf>) -> Self {
        Self {
            paths,
            resolved: None,
        }
    }

    /// Whether `candidate` canonicalizes onto one of these paths.
    ///
    /// `None` - a reported path that does not exist to resolve, most often a
    /// deleted file - matches nothing and does not force the set.
    fn contains(&mut self, candidate: Option<PathBuf>) -> bool {
        let Some(candidate) = candidate else {
            return false;
        };
        self.resolved
            .get_or_insert_with(|| {
                self.paths
                    .iter()
                    .filter_map(|path| path.canonicalize().ok())
                    .collect()
            })
            .contains(&candidate)
    }
}

/// Where the tool said the finding is, resolved against `root`.
///
/// The single definition of that, shared by `retain_requested` and the
/// `run_one` rewrite so the two comparisons cannot drift: a tool may report a
/// workspace-relative path or an absolute path, and only `root.join` of the
/// `./`-stripped form spells both the same way.
///
/// The *canonical* form each caller reaches for on a miss is deliberately not
/// computed here. It is the answer to a rarer question - a tool that resolved
/// symlinks or emitted `..` segments for its own cwd - and it costs a
/// filesystem walk per call, which on the common path buys nothing.
pub(crate) fn joined_reported(root: &Path, reported: &str) -> PathBuf {
    root.join(reported.strip_prefix("./").unwrap_or(reported))
}
