//! The repository that produced a diff answers the policy question for it.
//!
//! Staged and branch modes read every hunk from the object store of the
//! repository the run is rooted in, not from the working tree. Probing only
//! each hunk's working-tree directory would let a symbolic link there - a type
//! change the checked-out commit records, or one the working tree has not
//! staged - hand the policy question to whichever repository the link points
//! into, while the marked repository's committed source is sent.

#![cfg(unix)]

use std::path::Path;

use super::marker_refusal::{MARKER, repo};
use super::support::check_args;
use crate::cli::MachineFiles;
use crate::cli::check::{self, CheckArgs};
use crate::llm::cache::Cache;
use crate::test_support::{git_add, git_commit_all, git_init, git_output, request_count};

/// Run `args` in process against the policy file `site`.
async fn check_with(dir: &Path, site: &Path, args: &CheckArgs) -> check::Exit {
    check::run_against(
        args,
        dir,
        Cache::new(dir.join("test-cache"), 30, 8 * 1024 * 1024),
        &MachineFiles {
            auth: &dir.join("auth.toml"),
            policy: site,
        },
    )
    .await
    .expect("a refusal is a verdict, not a crash")
}

/// An unmarked repository a working-tree link can point into.
fn unmarked_repository() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("unmarked repository");
    git_init(dir.path());
    dir
}

/// The pre-push shape: the pushed tip holds `pkg/` as a directory while the
/// checked-out commit records `pkg` as a link into another repository.
#[tokio::test]
async fn a_branch_diff_is_refused_through_a_checked_out_directory_link() {
    let (dir, server, site) = repo(&[MARKER]).await;
    std::fs::write(dir.path().join(MARKER), "").expect("marker");
    git_commit_all(dir.path(), "base");
    let base = git_output(dir.path(), &["rev-parse", "HEAD"]);

    git_output(dir.path(), &["checkout", "--quiet", "-b", "feature"]);
    std::fs::create_dir(dir.path().join("pkg")).expect("pkg");
    std::fs::write(dir.path().join("pkg/a.py"), "secret = 1\n").expect("pkg/a.py");
    git_commit_all(dir.path(), "feature source");

    git_output(dir.path(), &["checkout", "--quiet", "main"]);
    let elsewhere = unmarked_repository();
    std::os::unix::fs::symlink(elsewhere.path(), dir.path().join("pkg")).expect("pkg link");
    git_commit_all(dir.path(), "pkg becomes a link");

    let mut args = check_args(Vec::new(), None);
    args.diff = Some(base);
    args.tip = Some("feature".to_owned());
    let exit = check_with(dir.path(), &site, &args).await;

    assert_eq!(exit, check::Exit::Unanalyzed);
    assert_eq!(
        request_count(&server).await,
        0,
        "the marked repository's committed source left the machine after only \
         the link target's repository was probed"
    );
}

/// The staged shape: the index holds `pkg/a.py` while the working tree has an
/// unstaged link at `pkg`.
#[tokio::test]
async fn a_staged_file_is_refused_through_an_unstaged_directory_link() {
    let (dir, server, site) = repo(&[MARKER]).await;
    std::fs::write(dir.path().join(MARKER), "").expect("marker");
    std::fs::create_dir(dir.path().join("pkg")).expect("pkg");
    std::fs::write(dir.path().join("pkg/a.py"), "secret = 1\n").expect("pkg/a.py");
    git_add(dir.path(), "pkg/a.py");
    std::fs::remove_file(dir.path().join("pkg/a.py")).expect("remove pkg/a.py");
    std::fs::remove_dir(dir.path().join("pkg")).expect("remove pkg");
    let elsewhere = unmarked_repository();
    std::os::unix::fs::symlink(elsewhere.path(), dir.path().join("pkg")).expect("pkg link");

    let mut args = check_args(Vec::new(), None);
    args.staged = true;
    let exit = check_with(dir.path(), &site, &args).await;

    assert_eq!(exit, check::Exit::Unanalyzed);
    assert_eq!(request_count(&server).await, 0);
}

/// The discriminating half: the same link layout in an unmarked repository
/// still reviews, so the refusal above comes from the marker and not from the
/// link.
#[tokio::test]
async fn the_same_staged_link_in_an_unmarked_repository_still_reviews() {
    let (dir, server, site) = repo(&[MARKER]).await;
    std::fs::create_dir(dir.path().join("pkg")).expect("pkg");
    std::fs::write(dir.path().join("pkg/a.py"), "x = 1\n").expect("pkg/a.py");
    git_add(dir.path(), "pkg/a.py");
    std::fs::remove_file(dir.path().join("pkg/a.py")).expect("remove pkg/a.py");
    std::fs::remove_dir(dir.path().join("pkg")).expect("remove pkg");
    let elsewhere = unmarked_repository();
    std::os::unix::fs::symlink(elsewhere.path(), dir.path().join("pkg")).expect("pkg link");

    let mut args = check_args(Vec::new(), None);
    args.staged = true;
    let exit = check_with(dir.path(), &site, &args).await;

    assert_eq!(exit, check::Exit::Clean);
    assert_eq!(request_count(&server).await, 1);
}
