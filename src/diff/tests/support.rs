//! Shared fixtures: a temp directory plus a real git repo so each test can
//! exercise one bit of git semantics.

use std::path::Path;

use tokio::process::Command;

/// A git repository rooted at a `tempfile::TempDir`.
///
/// Brings the repo into a state where every test is reproducible on every
/// CI runner: an identity, an initial commit, and the helper methods every
/// test needs to move files in or out of the index. Storing the `TempDir`
/// here (rather than `tempfile::tempdir()` inline) is what makes sure the
/// directory survives for the lifetime of the test.
pub(crate) struct GitRepo {
    pub dir: tempfile::TempDir,
}

impl GitRepo {
    /// `git init` a new directory, configure a local identity, and make one
    /// empty commit so subsequent `--cached`/`HEAD` queries behave like a
    /// real repo.
    pub async fn init() -> Self {
        let dir = tempfile::Builder::new()
            .prefix("drep-diff-test-")
            .tempdir()
            .expect("tempdir");
        let root = dir.path();

        // Use bin/sh-compatible forms because some CI runners expose a
        // minimal shell, and the cost of a non-zero exit here is "every
        // git-touching test fails mysteriously". `init.defaultBranch` is
        // pinned to `main` so branch-name-sensitive tests do not need to
        // guess the difference between pre- and post-2.28 git defaults.
        for argv in [
            vec!["init", "--quiet", "-b", "main"],
            vec!["config", "user.email", "drep@example.com"],
            vec!["config", "user.name", "drep"],
        ] {
            run_in(root, &argv).await;
        }

        let repo = Self { dir };
        repo.commit_all("initial").await;
        repo
    }

    /// `git init` without making any commits — for the empty-tree fallback
    /// test. The repo is a working repo, just one without history.
    pub async fn init_no_commits() -> Self {
        let dir = tempfile::Builder::new()
            .prefix("drep-diff-test-")
            .tempdir()
            .expect("tempdir");
        let root = dir.path();
        for argv in [
            vec!["init", "--quiet", "-b", "main"],
            vec!["config", "user.email", "drep@example.com"],
            vec!["config", "user.name", "drep"],
        ] {
            run_in(root, &argv).await;
        }
        Self { dir }
    }

    /// Stage every tracked change and create a commit with `message`.
    ///
    /// Used after each test that wants to advance the branch; tests like
    /// `changed_since_three_dot_does_not_include_base_changes` branch off
    /// this. `--allow-empty` is what lets `init()` create the seed commit
    /// on an empty repository — without it, the very first commit fails
    /// with "nothing to commit, working tree clean" and every test that
    /// depends on a HEAD panics in setup.
    pub async fn commit_all(&self, message: &str) {
        let root = self.dir.path();
        run_in(root, &["add", "."]).await;
        run_in(
            root,
            &[
                "-c",
                "commit.gpgsign=false",
                "commit",
                "--quiet",
                "--allow-empty",
                "-m",
                message,
            ],
        )
        .await;
    }

    /// Create `branch`, pointing at `HEAD`.
    pub async fn create_branch(&self, branch: &str) {
        run_in(self.dir.path(), &["branch", branch]).await;
    }

    /// Make `branch` the current branch.
    pub async fn checkout(&self, branch: &str) {
        run_in(self.dir.path(), &["checkout", "--quiet", branch]).await;
    }

    pub fn root(&self) -> &Path {
        self.dir.path()
    }
}

/// Run `git <args>` in `root`, swallowing non-zero exits.
///
/// Tests use this only for setup; assertion failures are surface as `Err`
/// from the API under test, not as panics from the setup command.
pub(crate) async fn run_in(root: &Path, args: &[&str]) {
    let mut command = Command::new("git");
    command.args(args).current_dir(root);
    let output = command.output().await.expect("spawn git");
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        panic!("git {args:?} in {} failed: {stderr}", root.display());
    }
}
