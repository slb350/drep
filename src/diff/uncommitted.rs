//! What a staged-mode linter would read that the commit does not hold.

use std::path::{Path, PathBuf};

use super::git::spawn_git;
use super::prefix::paths_from;
use super::{GitError, StagedView, filter_paths};

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
/// listing needs: quoting the name bytes, naming every file from the top
/// level, and ignoring external diff drivers and textconv. The untracked
/// listing takes `-- :/` for the same reason the diff takes `--no-relative`:
/// bare `ls-files` limits its answer to the working directory, so an
/// untracked helper above it would go unlisted.
pub async fn uncommitted_paths(root: &Path) -> Result<Vec<PathBuf>, GitError> {
    let view = StagedView::of(root).await?;
    let (tracked, untracked) = tokio::join!(
        spawn_git(
            root,
            &[
                // A read under a hook must not write: porcelain diff otherwise
                // rewrites the index it compared to refresh its stat data, and
                // that index can be the commit's own lock file.
                "--no-optional-locks",
                "-c",
                "core.quotePath=true",
                "diff",
                "--name-only",
                "--no-relative",
                "--no-ext-diff",
                "--no-textconv",
            ],
            view.env(),
        ),
        spawn_git(
            root,
            &[
                "--no-optional-locks",
                "-c",
                "core.quotePath=true",
                "ls-files",
                "--others",
                "--exclude-standard",
                "--full-name",
                "--",
                ":/",
            ],
            view.env(),
        )
    );
    let mut paths = paths_from(
        filter_paths(&tracked?, |_| true)
            .into_iter()
            .chain(filter_paths(&untracked?, |_| true))
            .collect(),
        &view.prefix,
    );
    paths.sort();
    paths.dedup();
    Ok(paths)
}
