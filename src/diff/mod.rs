//! Which files changed, in git terms.
//!
//! Every command in `drep` that looks at "what changed" comes through here.
//! `drep check --staged` for a pre-commit hook, `--diff <ref>` for a pre-push
//! gate, and the cache key that deduplicates repeated LLM calls all need a
//! stable answer to the same question. They shell out to git directly —
//! `tokio::process::Command` rather than libgit2, because the only operations
//! drep needs are the ones git's own CLI was built for, and a git CLI that
//! misbehaves would surface as a real OS-level error rather than a translated
//! library one.
//!
//! Two invariants matter more than the implementations:
//!
//! - "No files changed" must be **distinct** from "I could not ask git".
//!   Conflating them is how a commit gate rubber-stamps the day the user's
//!   git install breaks.
//! - `current_commit_sha` is the one place this is reversed: it only feeds
//!   a cache key, and a cache-key component must never take the analysis down.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use tokio::process::Command;

use crate::files;

pub mod hunks;

use hunks::{Hunk, parse_unified_diff};

/// The well-known SHA for the empty git tree.
///
/// On a fresh `git init` (no commits yet) there is no `HEAD` to diff against,
/// so drep diffs against this instead. Otherwise every first commit on a new
/// repository would fail with "fatal: ambiguous argument 'HEAD'".
const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

/// How long `current_commit_sha` is willing to wait for `git` to answer.
///
/// Used to be wall-clock unbounded, which was fine until a hung `git` stalled
/// the gate and blocked every commit. Five seconds is more than enough for a
/// local `git rev-parse`; if it does not answer by then the answer is
/// "unknown" and the cache key falls through.
const SHA_TIMEOUT: Duration = Duration::from_secs(5);

/// Ceiling on any single git invocation.
///
/// Generous compared with `SHA_TIMEOUT` because `git diff` on a large history
/// is legitimately slower than `rev-parse`, but bounded so a hung git cannot
/// stall a commit.
const GIT_TIMEOUT: Duration = Duration::from_secs(60);

/// What went wrong shelling out to git.
///
/// Distinct from `std::io::Error` because the most common cause — git exits
/// non-zero for "not a repository" — is not the same as "could not spawn
/// git", and the two should not be displayed the same way.
#[derive(Debug)]
pub enum GitError {
    /// `git` could not be spawned at all: missing binary, permission denied,
    /// or the OS rejected the argv. This is fundamentally different from
    /// git exiting non-zero.
    Spawn(String),
    /// `git` ran and refused. `code` is the exit status when git set one;
    /// `stderr` is the last argument worth showing to a user.
    NonZero { code: Option<i32>, stderr: String },
}

impl std::fmt::Display for GitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GitError::Spawn(msg) => write!(f, "git could not be spawned: {msg}"),
            GitError::NonZero { code, stderr } => {
                let code = code.map_or("<signal>".to_owned(), |c| c.to_string());
                write!(f, "git exited {code}: {stderr}")
            }
        }
    }
}

impl std::error::Error for GitError {}

/// Whether `root` has any commit yet.
///
/// Single-purpose helper: kept here because it is git semantics, not file
/// discovery, and the diff commands need to know this both for the empty-tree
/// fallback and for the `changed_since` no-HEAD case.
async fn has_head(root: &Path) -> bool {
    run_git(root, &["rev-parse", "--verify", "HEAD"])
        .await
        .is_ok()
}

/// Run `git <args>` in `root` and return trimmed stdout on success.
///
/// All the diff commands want the same shape: capture stdout, capture
/// stderr separately, never panic. `kill_on_drop` ensures a hung git cannot
/// outlive its caller.
/// Every git invocation is bounded.
///
/// The timeout lives here rather than at one call site: `current_commit_sha`
/// wrapped itself, but `staged_files`, `changed_since` and `has_head` called
/// this bare, so a hung git blocked the gate indefinitely. `kill_on_drop` only
/// helps when the future is dropped, which nothing was doing.
async fn run_git(root: &Path, args: &[&str]) -> Result<String, GitError> {
    let mut command = Command::new("git");
    command
        .args(args)
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let output = match tokio::time::timeout(GIT_TIMEOUT, command.output()).await {
        Ok(result) => result,
        Err(_) => {
            return Err(GitError::Spawn(format!(
                "git {} timed out after {}s",
                args.join(" "),
                GIT_TIMEOUT.as_secs()
            )));
        }
    }
    .map_err(|err| GitError::Spawn(err.to_string()))?;

    if !output.status.success() {
        return Err(GitError::NonZero {
            code: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// Parse the newline-delimited output of `git diff --name-only` into paths,
/// then keep only those drep analyzes.
///
/// Empty lines are tolerated because git occasionally emits a trailing one
/// depending on version and locale settings; the filter is the load-bearing
/// half — `is_scan_target` is what makes a diff query return files drep can
/// do something with, and keeps lock/build output from inflating the work
/// set.
fn filter_scan_targets(output: &str) -> Vec<PathBuf> {
    output
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(PathBuf::from)
        .filter(|path| files::is_scan_target(path))
        .collect()
}

/// Files staged for commit, relative to `root`, that drep analyzes.
///
/// `--diff-filter=ACMR` excludes deletions on purpose: a deleted file
/// cannot be analyzed, and passing it on would look like an unreadable file
/// rather than an absent one. The empty-tree fallback covers the
/// initial-commit case (no `HEAD` yet).
pub async fn staged_files(root: &Path) -> Result<Vec<PathBuf>, GitError> {
    let args: &[&str] = if has_head(root).await {
        &["diff", "--cached", "--name-only", "--diff-filter=ACMR"]
    } else {
        &[
            "diff",
            "--cached",
            "--name-only",
            "--diff-filter=ACMR",
            EMPTY_TREE,
        ]
    };
    let output = run_git(root, args).await?;
    Ok(filter_scan_targets(&output))
}

/// Files changed on this branch relative to `git_ref`, relative to `root`.
///
/// Three-dot diff (`<ref>...HEAD`) is the merge-base diff — *what my branch
/// changed*. Two-dot would also report everything that landed on the other
/// branch since the fork, which would gate a push on files the author never
/// touched.
///
/// `git_ref` is the same string the user typed: a branch name, a SHA, or a
/// remote-tracking ref like `origin/main`. A ref that does not exist makes
/// git exit non-zero, and that surfaces here as `Err(GitError::NonZero)`
/// rather than an empty Vec — see the module docs.
pub async fn changed_since(root: &Path, git_ref: &str) -> Result<Vec<PathBuf>, GitError> {
    let ref_b = if has_head(root).await {
        "HEAD"
    } else {
        EMPTY_TREE
    };
    let spec = format!("{git_ref}...{ref_b}");
    let output = run_git(root, &["diff", "--name-only", "--diff-filter=ACMR", &spec]).await?;
    Ok(filter_scan_targets(&output))
}

/// How many lines of unchanged context to request around each change.
///
/// Generous on purpose. The model has no parser and no whole-file view, so
/// this is the only thing giving it the surrounding function body to judge a
/// change against. git merges hunks whose context windows overlap, so a large
/// value cannot produce duplicate coverage of the same lines.
pub const CONTEXT_LINES: u32 = 20;

/// Hunks for the files staged for commit.
///
/// Same selection as `staged_files` — `--diff-filter=ACMR`, empty-tree
/// fallback when there is no HEAD — but the diff itself rather than the
/// names. `CONTEXT_LINES` of context is requested so the model reading each
/// hunk has the surrounding function body to compare against.
pub async fn staged_hunks(root: &Path) -> Result<Vec<Hunk>, GitError> {
    let unified = format!("--unified={CONTEXT_LINES}");
    let args: &[&str] = if has_head(root).await {
        &["diff", "--cached", "--diff-filter=ACMR", &unified]
    } else {
        &[
            "diff",
            "--cached",
            "--diff-filter=ACMR",
            &unified,
            EMPTY_TREE,
        ]
    };
    let output = run_git(root, args).await?;
    Ok(parse_unified_diff(&output))
}

/// Hunks for what this branch changed relative to `git_ref`.
///
/// Three-dot (`<ref>...HEAD`), matching `changed_since`: the merge-base diff,
/// so work that landed on the base branch after the fork is not attributed to
/// this branch. `CONTEXT_LINES` of context is requested so the model has
/// enough surrounding code to judge each change.
pub async fn hunks_since(root: &Path, git_ref: &str) -> Result<Vec<Hunk>, GitError> {
    let ref_b = if has_head(root).await {
        "HEAD"
    } else {
        EMPTY_TREE
    };
    let spec = format!("{git_ref}...{ref_b}");
    let unified = format!("--unified={CONTEXT_LINES}");
    let output = run_git(root, &["diff", "--diff-filter=ACMR", &unified, &spec]).await?;
    Ok(parse_unified_diff(&output))
}

/// The current commit's SHA, with `"unknown"` on any failure.
///
/// Deliberately lossy: this is the cache-key component for repeated LLM
/// calls, and a cache-key component must never take analysis down. A hung
/// git is a real failure mode in CI containers; the 5s timeout plus the
/// "unknown" fallback means the worst case is a cache miss, not a gate
/// stall.
pub async fn current_commit_sha(root: &Path) -> String {
    let result = tokio::time::timeout(SHA_TIMEOUT, run_git(root, &["rev-parse", "HEAD"])).await;
    match result {
        Ok(Ok(sha)) => normalize_sha(sha),
        // Timed out, git failed, or git is absent. All mean the same thing to a
        // cache key.
        _ => UNKNOWN_SHA.to_owned(),
    }
}

/// The placeholder used whenever the real SHA cannot be determined.
pub(crate) const UNKNOWN_SHA: &str = "unknown";

/// Empty output is as useless as an error.
///
/// Split out from `current_commit_sha` so it is reachable from a test: the
/// empty-success case cannot be provoked through real git, which fails rather
/// than succeeding with no output. Previously the guard sat inline and no test
/// could distinguish it from an unconditional pass-through.
pub(crate) fn normalize_sha(sha: String) -> String {
    if sha.is_empty() {
        UNKNOWN_SHA.to_owned()
    } else {
        sha
    }
}

#[cfg(test)]
mod tests;
