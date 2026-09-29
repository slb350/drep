//! What a staged-mode linter would read that the commit does not hold.

use std::path::{Path, PathBuf};

use super::git::spawn_git_bytes;
use super::prefix::{from_prefix, paths_from};
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
    let (tracked, untracked, flagged) = tokio::join!(
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
        ),
        flagged_differences(root, &view)
    );
    let (tracked, untracked) = (tracked?, untracked?);
    let mut paths = paths_from(
        listed(&tracked).chain(listed(&untracked)).collect(),
        &view.prefix,
    );
    paths.extend(flagged?);
    paths.sort();
    paths.dedup();
    Ok(paths)
}

/// Tracked files whose index entries tell git not to compare them
/// (`assume-unchanged`, `skip-worktree`) and whose working-tree state differs
/// from the committing index all the same, named from the directory drep runs
/// in. `git diff` trusts the flags and reports neither, so each flagged file
/// on disk is hashed and compared with its index blob, and anything else in
/// its place differs. One missing from the
/// working tree differs when assumed unchanged; a `skip-worktree` one missing
/// is a sparse checkout's ordinary state, which no tool can read.
async fn flagged_differences(root: &Path, view: &StagedView) -> Result<Vec<PathBuf>, GitError> {
    let entries = spawn_git_bytes(
        root,
        &[
            "--no-optional-locks",
            "ls-files",
            "-z",
            "--stage",
            "-v",
            "--full-name",
            "--",
            ":/",
        ],
        view.env(),
    )
    .await?;
    let mut differing = Vec::new();
    let mut present: Vec<(PathBuf, Vec<u8>)> = Vec::new();
    for (tag, blob, name) in entries.split(|&byte| byte == 0).filter_map(flagged_entry) {
        let skip_worktree = tag.eq_ignore_ascii_case(&b'S');
        let named = from_prefix(&path_from_bytes(name.to_vec()), &view.prefix);
        match std::fs::symlink_metadata(root.join(&named)) {
            Ok(meta) if meta.is_file() => present.push((named, blob.to_vec())),
            // A directory or a link in place of a flagged file is not the blob
            // the index holds, and hash-object would refuse the directory.
            Ok(_) => differing.push(named),
            Err(_) if !skip_worktree => differing.push(named),
            Err(_) => {}
        }
    }
    // A long list is hashed a batch at a time, within any system's argument limit.
    for batch in present.chunks(256) {
        let mut args: Vec<std::ffi::OsString> = vec!["hash-object".into(), "--".into()];
        args.extend(
            batch
                .iter()
                .map(|(named, _)| root.join(named).into_os_string()),
        );
        let hashes = spawn_git_bytes(root, &args, view.env()).await?;
        let mut hashes = hashes.split(|&byte| byte == b'\n');
        for (named, blob) in batch {
            if hashes.next() != Some(blob.as_slice()) {
                differing.push(named.clone());
            }
        }
    }
    Ok(differing)
}

/// One `ls-files --stage -v` record's tag, blob and name when the entry
/// carries a flag: a lowercase tag is assumed unchanged, and `S` or `s` is
/// skip-worktree.
fn flagged_entry(record: &[u8]) -> Option<(u8, &[u8], &[u8])> {
    let tab = record.iter().position(|&byte| byte == b'\t')?;
    let mut fields = record[..tab].split(|&byte| byte == b' ');
    let tag = *fields.next()?.first()?;
    let _mode = fields.next()?;
    let blob = fields.next()?;
    (tag.is_ascii_lowercase() || tag == b'S').then_some((tag, blob, &record[tab + 1..]))
}

/// The names in a NUL-separated git listing.
fn listed(output: &[u8]) -> impl Iterator<Item = PathBuf> + '_ {
    output
        .split(|&byte| byte == 0)
        .filter(|name| !name.is_empty())
        .map(|name| path_from_bytes(name.to_vec()))
}
