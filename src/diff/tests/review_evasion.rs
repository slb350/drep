//! Content that must not hide a change from review.
//!
//! Each test drives real `git` and asserts on the hunks the gate reviews, so a
//! regression in either the flags drep passes to `git diff` or the parser that
//! reads its output shows up as a change the reviewer never saw. Names and
//! configuration that reshape the patch are in `output_format.rs`.

use std::fs;
use std::path::Path;

use crate::diff::hunks::{Hunk, HunkLine};
use crate::diff::{hunks_since, run_git, staged_hunks};
use crate::files;

use super::support::{GitRepo, run_in};

/// Every added line the gate would review for `path`, in order.
fn added_lines(hunks: &[Hunk], path: &str) -> Vec<String> {
    hunks
        .iter()
        .filter(|hunk| hunk.file_path == Path::new(path))
        .flat_map(|hunk| &hunk.lines)
        .filter_map(|line| match line {
            HunkLine::Added(text) => Some(text.clone()),
            HunkLine::Context(_) | HunkLine::Removed(_) => None,
        })
        .collect()
}

async fn staged(root: &Path) -> Vec<Hunk> {
    staged_hunks(root, files::is_scan_target)
        .await
        .expect("staged_hunks")
}

#[tokio::test]
async fn an_added_line_spelling_a_deletion_header_does_not_end_the_review() {
    let repo = GitRepo::init().await;
    let root = repo.root();
    fs::write(
        root.join("lib.rs"),
        "fn reviewed() {}\nconst DOC: &str = r\"\n++ /dev/null\n\";\nfn hidden_from_review() {}\n",
    )
    .expect("write");
    run_in(root, &["add", "lib.rs"]).await;

    let added = added_lines(&staged(root).await, "lib.rs");

    assert_eq!(
        added,
        vec![
            "fn reviewed() {}",
            "const DOC: &str = r\"",
            "++ /dev/null",
            "\";",
            "fn hidden_from_review() {}",
        ]
    );
}

#[tokio::test]
async fn a_binary_attribute_does_not_hide_staged_source() {
    let repo = GitRepo::init().await;
    let root = repo.root();
    fs::write(root.join(".gitattributes"), "*.rs binary\n").expect("write");
    fs::write(root.join("evil.rs"), "fn payload() {}\n").expect("write");
    run_in(root, &["add", "."]).await;

    let added = added_lines(&staged(root).await, "evil.rs");

    assert_eq!(added, vec!["fn payload() {}"]);
}

#[tokio::test]
async fn a_binary_attribute_does_not_hide_pushed_source() {
    let repo = GitRepo::init().await;
    let root = repo.root();
    repo.create_branch("feature").await;
    repo.checkout("feature").await;
    fs::write(root.join(".gitattributes"), "*.rs -diff\n").expect("write");
    fs::write(root.join("evil.rs"), "fn payload() {}\n").expect("write");
    repo.commit_all("feature").await;

    let hunks = hunks_since(root, "main", files::is_scan_target)
        .await
        .expect("hunks_since");

    assert_eq!(added_lines(&hunks, "evil.rs"), vec!["fn payload() {}"]);
}

#[tokio::test]
async fn a_nul_byte_does_not_hide_staged_source() {
    // rustc accepts a NUL inside a comment; git's binary heuristic does not.
    let repo = GitRepo::init().await;
    let root = repo.root();
    fs::write(root.join("evil.rs"), "// \0\nfn payload() {}\n").expect("write");
    run_in(root, &["add", "evil.rs"]).await;

    let added = added_lines(&staged(root).await, "evil.rs");

    assert_eq!(added, vec!["// \0", "fn payload() {}"]);
}

#[tokio::test]
async fn a_symlink_replaced_by_source_is_reviewed() {
    // Stage the symlink through the index so the test needs no filesystem
    // symlink support; replacing it with a regular file is a type change.
    let repo = GitRepo::init().await;
    let root = repo.root();
    fs::write(root.join("target"), "elsewhere.rs").expect("write");
    let blob = run_git(root, &["hash-object", "-w", "target"])
        .await
        .expect("hash-object");
    fs::remove_file(root.join("target")).expect("remove");
    let cacheinfo = format!("120000,{blob},evil.rs");
    run_in(root, &["update-index", "--add", "--cacheinfo", &cacheinfo]).await;
    run_in(
        root,
        &[
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--quiet",
            "-m",
            "link",
        ],
    )
    .await;
    fs::write(root.join("evil.rs"), "fn payload() {}\n").expect("write");
    run_in(root, &["add", "evil.rs"]).await;

    let added = added_lines(&staged(root).await, "evil.rs");

    assert_eq!(added, vec!["fn payload() {}"]);
}
