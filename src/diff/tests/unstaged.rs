//! `unstaged_changes`: the tracked files a linter reading the working tree would see in a form the commit does not hold.

use std::fs;
use std::path::{Path, PathBuf};

use crate::diff::unstaged_changes;

use super::support::{GitRepo, run_in};

/// A git command against the fixture's repository that reads `index` as the
/// index. The env is set after `test_support::git` scrubbed the inherited one,
/// so the fixture's own alternate index - never a hook's - is what git sees.
async fn run_with_index(root: &Path, index: &Path, args: &[&str]) {
    let mut command = crate::test_support::git(root);
    command.args(args).env("GIT_INDEX_FILE", index);
    let output = command.output().expect("spawn git");
    assert!(
        output.status.success(),
        "git {args:?} in {} failed: {}",
        root.display(),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Edited-after-staging and deleted-from-the-working-tree both differ from
/// the index; a fully staged file and an untracked file do not.
#[tokio::test]
async fn lists_tracked_working_tree_differences_only() {
    let repo = GitRepo::init().await;
    let root = repo.root();

    fs::write(root.join("edited.py"), "a = 1\n").expect("write edited");
    fs::write(root.join("deleted.py"), "b = 1\n").expect("write deleted");
    fs::write(root.join("staged.py"), "c = 1\n").expect("write staged");
    run_in(root, &["add", "edited.py", "deleted.py", "staged.py"]).await;

    fs::write(root.join("edited.py"), "a = 2\n").expect("edit after staging");
    fs::remove_file(root.join("deleted.py")).expect("delete from the working tree");
    fs::write(root.join("untracked.py"), "d = 1\n").expect("write untracked");

    let paths = unstaged_changes(root).await.expect("unstaged_changes");
    assert_eq!(
        paths,
        vec![PathBuf::from("deleted.py"), PathBuf::from("edited.py")],
        "edited and deleted tracked files, nothing else"
    );
}

/// The index read is the one the commit is made from, not `.git/index`.
///
/// A hook's `GIT_INDEX_FILE` is process-global state a test cannot set from
/// inside the suite, so the assertion runs in a child test process under the
/// environment a hook would export. The direct call beside it pins the other
/// half of the causality: the same working tree against the ordinary index is
/// clean, so only the committing index can have produced the listing.
#[tokio::test]
async fn reads_the_committing_index_a_hook_names() {
    let repo = GitRepo::init().await;
    let root = repo.root();
    fs::write(root.join("staged.py"), "a = 1\n").expect("write");
    repo.commit_all("track staged.py").await;

    // An alternate index holding an edit the working tree no longer holds:
    // seed it from HEAD, stage the edit into it, then restore the file.
    let alternate = root.join(".git").join("alternate.index");
    run_with_index(root, &alternate, &["read-tree", "HEAD"]).await;
    fs::write(root.join("staged.py"), "a = 2\n").expect("edit");
    run_with_index(root, &alternate, &["add", "staged.py"]).await;
    fs::write(root.join("staged.py"), "a = 1\n").expect("restore");

    assert!(
        unstaged_changes(root)
            .await
            .expect("ordinary index")
            .is_empty(),
        "the working tree matches the ordinary index"
    );

    let output = std::process::Command::new(std::env::current_exe().expect("this test binary"))
        .args([
            "diff::tests::unstaged::under_a_hook_names_the_alternate_index",
            "--exact",
            "--test-threads=1",
        ])
        .env("GIT_DIR", root.join(".git"))
        .env("GIT_INDEX_FILE", &alternate)
        .env("DREP_TEST_ALTERNATE_INDEX_ROOT", root)
        .output()
        .expect("run the hook-environment test");
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("1 passed"),
        "the committing index was not read: {output:?}"
    );
}

/// The hook half of `reads_the_committing_index_a_hook_names`. Only the parent
/// can put `GIT_INDEX_FILE` into the environment, so a bare suite run has no
/// alternate index to read and nothing to assert.
#[tokio::test]
async fn under_a_hook_names_the_alternate_index() {
    let Some(root) = std::env::var_os("DREP_TEST_ALTERNATE_INDEX_ROOT") else {
        return;
    };
    let paths = unstaged_changes(Path::new(&root))
        .await
        .expect("unstaged_changes");
    assert_eq!(
        paths,
        vec![PathBuf::from("staged.py")],
        "the committing index holds an edit the working tree does not"
    );
}

/// Run from a subdirectory, each differing file is named from there.
#[tokio::test]
async fn names_paths_from_the_subdirectory() {
    let repo = GitRepo::init().await;
    let root = repo.root();
    fs::write(root.join("top.py"), "a = 1\n").expect("write");
    run_in(root, &["add", "top.py"]).await;
    fs::write(root.join("top.py"), "a = 2\n").expect("edit after staging");
    fs::create_dir_all(root.join("sub")).expect("subdirectory");

    let paths = unstaged_changes(&root.join("sub"))
        .await
        .expect("unstaged_changes");
    assert_eq!(paths, vec![PathBuf::from("../top.py")]);
}
