//! What a staged-mode linter would read that the commit does not hold.

use std::path::{Path, PathBuf};

use super::git::{GitEnv, spawn_git_bytes, spawn_git_bytes_with_input};
use super::prefix::{from_prefix, paths_from};
use super::quoting::path_from_bytes;
use super::{GitError, StagedView};

/// What the working tree does not hold of the committing index, each path
/// named from the directory drep runs in.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Uncommitted {
    /// Files whose working-tree state the committing index does not hold:
    /// tracked files whose working-tree content differs from that index,
    /// untracked files git does not ignore, and flagged files edited behind
    /// their flags. This is what a linter reading the working tree would see
    /// that the commit does not hold.
    pub differing: Vec<PathBuf>,
    /// Files marked `skip-worktree` and absent from the working tree, as a
    /// sparse checkout leaves them. No tool can read one, so none refuses a
    /// tool that reads beyond its files; but one can be the configuration
    /// the commit gives a file whose working tree shows none.
    pub index_only: Vec<PathBuf>,
}

/// The committing index's paths the working tree does not hold.
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
pub async fn uncommitted_paths(root: &Path) -> Result<Uncommitted, GitError> {
    let view = StagedView::of(root).await?;
    // `-z`: a name is listed unquoted, with a leading space or a newline of
    // its own intact.
    let (tracked, untracked, listing) = tokio::join!(
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
        spawn_git_bytes(
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
    );
    let (tracked, untracked) = (tracked?, untracked?);
    let mut paths = paths_from(
        listed(&tracked).chain(listed(&untracked)).collect(),
        &view.prefix,
    );
    let listing = listing?;
    let (flagged, filtered) = tokio::join!(
        flagged_differences(root, &view, &listing),
        filtered_differences(root, &view, &listing)
    );
    let flagged = flagged?;
    paths.extend(filtered?);
    paths.extend(flagged.differing);
    paths.sort();
    paths.dedup();
    Ok(Uncommitted {
        differing: paths,
        index_only: flagged.index_only,
    })
}

/// Tracked files whose index entries tell git not to compare them
/// (`assume-unchanged`, `skip-worktree`) and whose working-tree state differs
/// from the committing index all the same. `git diff` trusts the flags and
/// reports neither, so each flagged regular file on disk is hashed and
/// compared with its index blob, a flagged link's text is compared with its
/// blob, a flagged submodule's checkout with its recorded commit
/// (`submodule_differs`), and anything else in its place differs. One missing from the working
/// tree differs when assumed unchanged; a `skip-worktree` one missing is a
/// sparse checkout's ordinary state and is index-only.
async fn flagged_differences(
    root: &Path,
    view: &StagedView,
    listing: &[u8],
) -> Result<Uncommitted, GitError> {
    const LINK: &[u8] = b"120000";
    const GITLINK: &[u8] = b"160000";
    let mut flagged = Uncommitted::default();
    let mut files: Vec<(PathBuf, &[u8])> = Vec::new();
    for entry in index_entries(listing).filter(IndexEntry::flagged) {
        let named = from_prefix(&path_from_bytes(entry.name.to_vec()), &view.prefix);
        if entry.mode == GITLINK {
            if submodule_differs(&root.join(&named), entry.blob).await? {
                flagged.differing.push(named);
            }
            continue;
        }
        match std::fs::symlink_metadata(root.join(&named)) {
            Ok(meta) if meta.is_file() && entry.mode != LINK => files.push((named, entry.blob)),
            Ok(meta) if meta.file_type().is_symlink() && entry.mode == LINK => {
                // A link's blob is its target text.
                let oid = String::from_utf8_lossy(entry.blob);
                let text = spawn_git_bytes(root, &["cat-file", "blob", &oid], view.env()).await?;
                if !std::fs::read_link(root.join(&named))
                    .is_ok_and(|target| target == path_from_bytes(text))
                {
                    flagged.differing.push(named);
                }
            }
            // Anything else in a flagged file's place is not the blob the
            // index holds, and hash-object would refuse a directory.
            Ok(_) => flagged.differing.push(named),
            Err(_) if entry.skip_worktree() => flagged.index_only.push(named),
            Err(_) => flagged.differing.push(named),
        }
    }
    // A long list is hashed a batch at a time, within any system's argument limit.
    for batch in files.chunks(256) {
        let mut args: Vec<std::ffi::OsString> = vec!["hash-object".into(), "--".into()];
        args.extend(
            batch
                .iter()
                .map(|(named, _)| root.join(named).into_os_string()),
        );
        let hashes = spawn_git_bytes(root, &args, view.env()).await?;
        let mut hashes = hashes.split(|&byte| byte == b'\n');
        for (named, blob) in batch {
            if hashes.next() != Some(*blob) {
                flagged.differing.push(named.clone());
            }
        }
    }
    Ok(flagged)
}

/// Whether a flagged submodule's checkout differs from the commit its index
/// entry records: another commit checked out, changes of its own, or
/// something other than a directory in its place. A submodule not checked
/// out, or never initialized, holds nothing a tool can read.
async fn submodule_differs(path: &Path, commit: &[u8]) -> Result<bool, GitError> {
    match std::fs::symlink_metadata(path) {
        Err(_) => return Ok(false),
        Ok(meta) if !meta.is_dir() => return Ok(true),
        Ok(_) => {}
    }
    if !path.join(".git").exists() {
        return Ok(false);
    }
    let head = spawn_git_bytes(path, &["rev-parse", "HEAD"], GitEnv::Scrubbed).await?;
    if head.trim_ascii() != commit {
        return Ok(true);
    }
    let changes = spawn_git_bytes(
        path,
        &[
            "--no-optional-locks",
            "status",
            "--porcelain",
            "-z",
            "--ignore-submodules=none",
        ],
        GitEnv::Scrubbed,
    )
    .await?;
    Ok(!changes.is_empty())
}

/// Tracked regular files a clean filter (a `filter` attribute) rewrites whose
/// working-tree bytes differ from what checking out their index blob writes.
/// `git diff` compares the filtered content and reports none of them, while a
/// linter reads the file as it is: a filter that strips a suppression comment
/// commits a file the linter never saw. Git LFS is left alone: it round-trips
/// losslessly and is content-addressed, so an LFS file `git diff` reports
/// unchanged already holds its blob's content, and smudging every asset again
/// would stream them all on each commit.
async fn filtered_differences(
    root: &Path,
    view: &StagedView,
    listing: &[u8],
) -> Result<Vec<PathBuf>, GitError> {
    let regular: Vec<IndexEntry<'_>> = index_entries(listing)
        .filter(|entry| entry.mode == b"100644" || entry.mode == b"100755")
        .collect();
    if regular.is_empty() {
        return Ok(Vec::new());
    }
    let named: Vec<PathBuf> = regular
        .iter()
        .map(|entry| from_prefix(&path_from_bytes(entry.name.to_vec()), &view.prefix))
        .collect();
    let mut input = Vec::new();
    for path in &named {
        input.extend_from_slice(path.as_os_str().as_encoded_bytes());
        input.push(0);
    }
    let attributes = spawn_git_bytes_with_input(
        root,
        &["check-attr", "--stdin", "-z", "filter"],
        view.env(),
        &input,
    )
    .await?;
    // `path NUL filter NUL value NUL`, one record per path in the order given.
    let values = attributes.split(|&byte| byte == 0).skip(2).step_by(3);
    let mut differing = Vec::new();
    for ((entry, path), value) in regular.iter().zip(named).zip(values) {
        if matches!(value, b"unspecified" | b"unset" | b"set" | b"lfs") {
            continue;
        }
        // A file absent from the working tree is `git diff`'s to report.
        let Ok(raw) = std::fs::read(root.join(&path)) else {
            continue;
        };
        let mut at = std::ffi::OsString::from("--path=");
        at.push(&path);
        let oid = std::ffi::OsString::from(String::from_utf8_lossy(entry.blob).as_ref());
        let checkout = spawn_git_bytes(
            root,
            &[
                std::ffi::OsString::from("cat-file"),
                "--filters".into(),
                at,
                oid,
            ],
            view.env(),
        )
        .await;
        if checkout.is_ok_and(|content| content == raw) {
            continue;
        }
        differing.push(path);
    }
    Ok(differing)
}

/// One `ls-files --stage -v` record.
struct IndexEntry<'a> {
    /// A lowercase tag is assumed unchanged, and `S` or `s` is skip-worktree.
    tag: u8,
    mode: &'a [u8],
    blob: &'a [u8],
    name: &'a [u8],
}

impl IndexEntry<'_> {
    /// Whether git is told not to compare this entry with the working tree.
    fn flagged(&self) -> bool {
        self.tag.is_ascii_lowercase() || self.tag == b'S'
    }

    fn skip_worktree(&self) -> bool {
        self.tag.eq_ignore_ascii_case(&b'S')
    }
}

/// The records of an `ls-files --stage -v -z` listing.
fn index_entries(listing: &[u8]) -> impl Iterator<Item = IndexEntry<'_>> {
    listing.split(|&byte| byte == 0).filter_map(|record| {
        let tab = record.iter().position(|&byte| byte == b'\t')?;
        let mut fields = record[..tab].split(|&byte| byte == b' ');
        Some(IndexEntry {
            tag: *fields.next()?.first()?,
            mode: fields.next()?,
            blob: fields.next()?,
            name: &record[tab + 1..],
        })
    })
}

/// The names in a NUL-separated git listing.
fn listed(output: &[u8]) -> impl Iterator<Item = PathBuf> + '_ {
    output
        .split(|&byte| byte == 0)
        .filter(|name| !name.is_empty())
        .map(|name| path_from_bytes(name.to_vec()))
}
