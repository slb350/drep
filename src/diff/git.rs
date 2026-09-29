//! The one place drep spawns git, and the environment each spawn sees.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use super::GitError;

/// Ceiling on any single git invocation.
///
/// Generous compared with `SHA_TIMEOUT` because `git diff` on a large history
/// is legitimately slower than `rev-parse`, but bounded so a hung git cannot
/// stall a commit.
const GIT_TIMEOUT: Duration = Duration::from_secs(60);

/// Run a git query whose answer is carried by its exit code.
///
/// `Ok(Some(stdout))` when git exited 0, `Ok(None)` when it exited **1**, and
/// an error for anything else. Exit 1 is git's "no" - not ignored, not tracked,
/// no such config key - while 2 and above mean the question could not be asked
/// at all, and collapsing the two would report a broken repository as a clean
/// answer.
///
/// Three call sites had transcribed this discrimination separately
/// (`hooks::run_git_config_path`, and `gitignore`'s ignored and tracked
/// probes), which is three places for the 1-versus-2 rule to drift.
pub(crate) async fn git_query(root: &Path, args: &[&str]) -> Result<Option<String>, GitError> {
    match run_git(root, args).await {
        Ok(stdout) => Ok(Some(stdout)),
        Err(GitError::NonZero { code: Some(1), .. }) => Ok(None),
        Err(err) => Err(err),
    }
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
///
/// `pub(crate)` because it and the staged diff, through [`spawn_git`], are the
/// *only* place drep spawns git. `cli::init` asks git where the hooks directory
/// is and what `core.hooksPath` holds, and a second spawn helper there would be
/// a second place for the timeout, the stdin-null and the non-zero handling to
/// drift.
pub(crate) async fn run_git(root: &Path, args: &[&str]) -> Result<String, GitError> {
    spawn_git(root, args, GitEnv::Scrubbed).await
}

/// How a spawned git sees the environment drep inherited.
#[derive(Clone, Copy)]
pub(super) enum GitEnv<'a> {
    /// Every inherited repository variable removed, so the directory drep names is the repository git answers about.
    Scrubbed,
    /// Scrubbed, then reading this file as the index: the index a hook's commit is being made from.
    Index(&'a Path),
    /// As inherited: git's view of the repository the hook that launched drep runs in.
    Inherited,
}

/// The index a hook's `git commit` is being made from, when that hook runs in `root`'s repository.
///
/// git names it to a pre-commit hook in `GIT_INDEX_FILE`: `.git/index` after
/// `git add`, a temporary one for `git commit -a` or `git commit <paths>`, or
/// wherever the committer pointed it. The staged diff reads it, made absolute
/// against drep's working directory, when git under the hook's own environment
/// names `root`'s git directory; otherwise it reads `root`'s index as every other
/// query does.
pub(super) async fn committing_index(root: &Path) -> Result<Option<PathBuf>, GitError> {
    let Some(inherited) = std::env::var_os("GIT_INDEX_FILE") else {
        return Ok(None);
    };
    let (ours, hooks) = tokio::join!(
        spawn_git(root, &["rev-parse", "--absolute-git-dir"], GitEnv::Scrubbed),
        spawn_git(
            Path::new("."),
            &["rev-parse", "--absolute-git-dir"],
            GitEnv::Inherited
        ),
    );
    let (Ok(ours), Ok(hooks)) = (ours, hooks) else {
        return Ok(None);
    };
    owned_index(&inherited, Path::new(&hooks), Path::new(&ours))
}

/// `index`, made absolute, when the hook that named it runs in the repository whose git directory is `git_dir`.
///
/// A hook in another repository names an index drep must not read. One in this
/// repository whose index cannot be resolved fails the staged review rather
/// than letting it read a different index.
pub(super) fn owned_index(
    index: &OsStr,
    hook_git_dir: &Path,
    git_dir: &Path,
) -> Result<Option<PathBuf>, GitError> {
    if !same_directory(hook_git_dir, git_dir) {
        return Ok(None);
    }
    std::path::absolute(index).map(Some).map_err(|err| {
        GitError::Spawn(format!(
            "cannot resolve the index this commit is made from ({}): {err}",
            Path::new(index).display()
        ))
    })
}

/// Whether two paths name the same directory: equal as written, or once both are canonicalized.
pub(crate) fn same_directory(left: &Path, right: &Path) -> bool {
    left == right
        || std::fs::canonicalize(left)
            .and_then(|left| std::fs::canonicalize(right).map(|right| left == right))
            .unwrap_or(false)
}

/// Spawn git in `root` with the environment `env` describes, and return its trimmed output.
pub(super) async fn spawn_git(
    root: &Path,
    args: &[&str],
    env: GitEnv<'_>,
) -> Result<String, GitError> {
    let stdout = spawn_git_bytes(root, args, env).await?;
    Ok(String::from_utf8_lossy(&stdout).trim().to_owned())
}

/// [`spawn_git`]'s output as git wrote it: a blob's content is neither trimmed nor decoded.
pub(super) async fn spawn_git_bytes<S: AsRef<OsStr>>(
    root: &Path,
    args: &[S],
    env: GitEnv<'_>,
) -> Result<Vec<u8>, GitError> {
    spawn_git_io(root, args, env, None).await
}

/// [`spawn_git_bytes`] with `input` on git's standard input, for a query that
/// takes its paths there (`--stdin`) rather than on an argument list whose
/// length the system bounds.
pub(super) async fn spawn_git_bytes_with_input<S: AsRef<OsStr>>(
    root: &Path,
    args: &[S],
    env: GitEnv<'_>,
    input: &[u8],
) -> Result<Vec<u8>, GitError> {
    spawn_git_io(root, args, env, Some(input)).await
}

/// Every spawn of git runs here: the environment scrubbed as `env` says,
/// `input` (if any) on its standard input, and one timeout over the whole run.
async fn spawn_git_io<S: AsRef<OsStr>>(
    root: &Path,
    args: &[S],
    env: GitEnv<'_>,
    input: Option<&[u8]>,
) -> Result<Vec<u8>, GitError> {
    let mut command = Command::new("git");
    command
        .args(args)
        .current_dir(root)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if !matches!(env, GitEnv::Inherited) {
        command
            // drep names the repository by path, and `current_dir(root)` is that
            // statement. An inherited `GIT_DIR`/`GIT_WORK_TREE`/`GIT_COMMON_DIR`
            // silently overrides it, so git answers about a *different* repository
            // than the one asked about - and a relative `GIT_INDEX_FILE` resolves
            // against the wrong directory entirely. Both happen in practice,
            // because drep's whole job is running inside a git hook, where git
            // exports all of them.
            //
            // Removing them makes `root` authoritative. It changes nothing in the
            // ordinary case (git rediscovers the same repository from the working
            // directory), and it is what stops the answers depending on who
            // launched the process.
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_COMMON_DIR")
            .env_remove("GIT_INDEX_FILE")
            // The object-database trio, for the same reason as the four above:
            // they redirect where a child `git` reads and writes objects, so an
            // inherited one points at the outer repository's store while every
            // other setting names the intended one.
            .env_remove("GIT_OBJECT_DIRECTORY")
            .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES")
            .env_remove("GIT_QUARANTINE_PATH");
    }
    if let GitEnv::Index(index) = env {
        command.env("GIT_INDEX_FILE", index);
    }

    let run = async {
        let mut child = command.spawn()?;
        let stdin = child.stdin.take();
        // The input is written while the output is read, so neither pipe can
        // fill and stall the other. A failed write means git stopped reading,
        // and its exit status says why.
        let feed = async {
            if let (Some(mut stdin), Some(input)) = (stdin, input) {
                let _ = stdin.write_all(input).await;
            }
        };
        let ((), output) = tokio::join!(feed, child.wait_with_output());
        output
    };
    let output = match tokio::time::timeout(GIT_TIMEOUT, run).await {
        Ok(result) => result,
        Err(_) => {
            return Err(GitError::Spawn(format!(
                "git {} timed out after {}s",
                args.iter()
                    .map(|arg| arg.as_ref().to_string_lossy())
                    .collect::<Vec<_>>()
                    .join(" "),
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
    Ok(output.stdout)
}
