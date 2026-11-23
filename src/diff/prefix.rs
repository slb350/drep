//! Diff paths as seen from the directory drep was run in.
//!
//! Every staged and branch query names its files from the top level of the
//! working tree (`--no-relative` in `DIFF`), while drep's commands read, report
//! and hand on each file relative to the directory they run in. Run from a
//! subdirectory, `sub/doc.md` joined onto that directory named a file that does
//! not exist, so each query's answer is converted here before it is returned.

use std::path::{Component, Path, PathBuf};

use super::GitError;
use super::git::run_git;
use super::hunks::Hunk;

/// Where `root` sits below the top level of its working tree, as
/// `git rev-parse --show-prefix` prints it: empty at the top level, `sub/dir/`
/// below it.
pub(super) async fn working_prefix(root: &Path) -> Result<PathBuf, GitError> {
    Ok(PathBuf::from(
        run_git(root, &["rev-parse", "--show-prefix"]).await?,
    ))
}

/// `path`, named from the top level of the working tree, as seen from the
/// directory `prefix` names: from `sub/`, `sub/doc.md` is `doc.md` and
/// `README.md` is `../README.md`.
pub(super) fn from_prefix(path: &Path, prefix: &Path) -> PathBuf {
    let mut prefix_components = prefix.components();
    let mut path_components = path.components();
    let mut out = PathBuf::new();
    loop {
        match (prefix_components.next(), path_components.next()) {
            (Some(left), Some(right)) if left == right => continue,
            (Some(_), right) => {
                out.push(Component::ParentDir);
                for _ in prefix_components {
                    out.push(Component::ParentDir);
                }
                if let Some(right) = right {
                    out.push(right);
                }
                out.extend(path_components);
                return out;
            }
            (None, Some(right)) => {
                out.push(right);
                out.extend(path_components);
                return out;
            }
            (None, None) => return out,
        }
    }
}

/// Each of `paths` as seen from `prefix`.
pub(super) fn paths_from(paths: Vec<PathBuf>, prefix: &Path) -> Vec<PathBuf> {
    paths.iter().map(|path| from_prefix(path, prefix)).collect()
}

/// Each of `hunks` with its file as seen from `prefix`.
pub(super) fn hunks_from(mut hunks: Vec<Hunk>, prefix: &Path) -> Vec<Hunk> {
    for hunk in &mut hunks {
        hunk.file_path = from_prefix(&hunk.file_path, prefix);
    }
    hunks
}
