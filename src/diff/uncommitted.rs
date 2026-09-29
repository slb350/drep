//! What a staged-mode linter would read that the commit does not hold.

use std::path::{Path, PathBuf};

use super::git::spawn_git_bytes;
use super::prefix::paths_from;
use super::quoting::path_from_bytes;
use super::{GitError, StagedView};

/// Files whose working-tree state the committing index does not hold, named from the directory drep runs in: tracked files whose working-tree content differs from that index, plus untracked files git does not ignore. This is what a linter reading the working tree would see that the commit does not hold.
///
/// The deterministic layer's tools read the working tree, so after a partial
/// `git add -p`, an edit made after staging, or a source file never added at
/// all they would lint content the commit does not contain - and a
/// lint-failing staged blob could pass. This listing is what lets the caller
/// refuse those runs. The index read is the committing one
/// (`StagedView::env`), so a hook's alternate `GIT_INDEX_FILE` is honoured
/// the same way it is for the staged diff itself.
///
/// The diff argv is deliberately not `DIFF`: that set carries hunk-only pins
/// and `--diff-filter=ACMRT`, which would drop the deletions this query
/// exists to see (a staged file deleted from the working tree differs
/// exactly as an edited one does). What stays is the subset a name-only
/// listing needs: naming every file from the top level and ignoring external
/// diff drivers and textconv. Both listings are NUL-separated and read as raw
/// bytes, never through `spawn_git`, which trims what it reads, so each name
/// is taken byte for byte. The untracked listing takes `-- :/` for the same reason the diff takes `--no-relative`:
/// bare `ls-files` limits its answer to the working directory, so an
/// untracked helper above it would go unlisted.
pub async fn uncommitted_paths(root: &Path) -> Result<Vec<PathBuf>, GitError> {
    let view = StagedView::of(root).await?;
    // `-z`: a name is listed unquoted, with a leading space or a newline of
    // its own intact.
    let (tracked, untracked) = tokio::join!(
        spawn_git_bytes(
            root,
            &[
                // A read under a hook must not write: porcelain diff otherwise
                // rewrites the index it compared to refresh its stat data, and
                // that index can be the commit's own lock file.
                "--no-optional-locks",
                "diff",
                "--name-only",
                "-z",
                // A dirty submodule a staged file builds against differs even
                // where configuration says to ignore submodules.
                "--ignore-submodules=none",
                "--no-relative",
                "--no-ext-diff",
                "--no-textconv",
            ],
            view.env(),
        ),
        spawn_git_bytes(
            root,
            &[
                "--no-optional-locks",
                "ls-files",
                "-z",
                "--others",
                "--exclude-standard",
                "--full-name",
                "--",
                ":/",
            ],
            view.env(),
        )
    );
    let (tracked, untracked) = (tracked?, untracked?);
    let mut paths = paths_from(
        listed(&tracked).chain(listed(&untracked)).collect(),
        &view.prefix,
    );
    paths.sort();
    paths.dedup();
    Ok(paths)
}

/// The names in a NUL-separated git listing.
fn listed(output: &[u8]) -> impl Iterator<Item = PathBuf> + '_ {
    output
        .split(|&byte| byte == 0)
        .filter(|name| !name.is_empty())
        .map(|name| path_from_bytes(name.to_vec()))
}
