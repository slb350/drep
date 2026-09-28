//! Run from a subdirectory, each staged and branch query covers the whole commit or branch and names every file from that directory, whatever `diff.relative` says.

use std::fs;
use std::path::{Path, PathBuf};

use crate::diff::{changed_since, hunks_since, staged_files, staged_hunks};
use crate::files;

use super::support::{GitRepo, RELATIVE, run_in};

/// What each query must answer from `sub/`: the subdirectory's file by its own name, and the top level's by climbing to it, in git's order.
fn expected() -> Vec<PathBuf> {
    vec![PathBuf::from("inner.rs"), PathBuf::from("../top.rs")]
}

/// Writes `top.rs` at the top level and `sub/inner.rs` below it and sets `diff.relative`.
fn write_both(root: &Path) {
    fs::create_dir_all(root.join("sub")).expect("subdirectory");
    fs::write(root.join("top.rs"), "fn top() {}\n").expect("write");
    fs::write(root.join("sub").join("inner.rs"), "fn inner() {}\n").expect("write");
}

/// Each path names a real file from `sub`.
fn assert_real(sub: &Path, paths: &[PathBuf], relative: &str) {
    for path in paths {
        assert!(
            sub.join(path).is_file(),
            "{} does not name a file from sub/ (diff.relative={relative})",
            path.display()
        );
    }
}

#[tokio::test]
async fn staged_files_and_hunks_are_named_from_the_subdirectory() {
    for relative in RELATIVE {
        let repo = GitRepo::init().await;
        let root = repo.root();
        write_both(root);
        run_in(root, &["add", "top.rs", "sub/inner.rs"]).await;
        run_in(root, &["config", "--local", "diff.relative", relative]).await;
        let sub = root.join("sub");

        let names = staged_files(&sub, files::is_scan_target)
            .await
            .expect("staged_files");
        assert_eq!(names, expected(), "diff.relative={relative}");
        assert_real(&sub, &names, relative);

        let hunks = staged_hunks(&sub, files::is_scan_target)
            .await
            .expect("staged_hunks");
        let hunk_files: Vec<PathBuf> = hunks.into_iter().map(|hunk| hunk.file_path).collect();
        assert_eq!(hunk_files, expected(), "diff.relative={relative}");
    }
}

#[tokio::test]
async fn branch_changes_and_hunks_are_named_from_the_subdirectory() {
    for relative in RELATIVE {
        let repo = GitRepo::init().await;
        let root = repo.root();
        repo.create_branch("feature").await;
        repo.checkout("feature").await;
        write_both(root);
        repo.commit_all("both").await;
        run_in(root, &["config", "--local", "diff.relative", relative]).await;
        let sub = root.join("sub");

        let names = changed_since(&sub, "main").await.expect("changed_since");
        assert_eq!(names, expected(), "diff.relative={relative}");
        assert_real(&sub, &names, relative);

        let hunks = hunks_since(&sub, "main", files::is_scan_target)
            .await
            .expect("hunks_since");
        let hunk_files: Vec<PathBuf> = hunks.into_iter().map(|hunk| hunk.file_path).collect();
        assert_eq!(hunk_files, expected(), "diff.relative={relative}");
    }
}
