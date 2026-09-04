//! Narrow a whole-project tool's findings to the files drep was asked to check.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::analysis::findings::Finding;
use crate::languages::spec::ToolSpec;

/// Narrow a tool's findings to the files actually being checked.
///
/// A whole-project tool (`cargo clippy`, `tsc`, `dotnet format`) reports on
/// everything it compiled, and a commit gate that blocked on pre-existing
/// issues in untouched code would be unusable: the author cannot fix what they
/// did not write, and every commit would fail until the whole project was
/// clean.
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
    workspace_root: &Path,
) -> Vec<Finding> {
    if spec.accepts_files || findings.is_empty() {
        return findings;
    }
    let wanted: BTreeSet<PathBuf> = files
        .iter()
        .map(|file| joined_reported(workspace_root, file))
        .collect();
    let mut wanted_canonical = LazyCanonical::of(&wanted);
    findings
        .into_iter()
        .filter(|finding| {
            let joined = joined_reported(workspace_root, &finding.file_path);
            wanted.contains(&joined) || wanted_canonical.contains(joined.canonicalize().ok())
        })
        .collect()
}

/// The canonical forms of a set of paths, resolved on first use.
///
/// Every member costs a `realpath`, so the set is worth building only once
/// something has failed to match in exact form - which under an ordinary
/// checkout is never.
struct LazyCanonical<'a> {
    paths: &'a BTreeSet<PathBuf>,
    resolved: Option<BTreeSet<PathBuf>>,
}

impl<'a> LazyCanonical<'a> {
    fn of(paths: &'a BTreeSet<PathBuf>) -> Self {
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
