//! `staged_contents`: each staged file with the content the commit records, read from the index.

use std::fs;
use std::path::PathBuf;

use crate::diff::staged_contents;
use crate::files;

use super::support::{GitRepo, run_in};

#[tokio::test]
async fn the_content_is_the_indexs_exactly_whatever_the_working_tree_holds() {
    let repo = GitRepo::init().await;
    let root = repo.root();
    let staged = "# Staged\n\nThe commit records this.\n\n";
    fs::write(root.join("guide.md"), staged).expect("write");
    fs::write(root.join("main.rs"), "fn main() {}\n").expect("write");
    run_in(root, &["add", "guide.md", "main.rs"]).await;
    fs::write(root.join("guide.md"), "# Working tree\n").expect("rewrite");

    let contents = staged_contents(root, files::is_markdown)
        .await
        .expect("staged_contents");

    assert_eq!(contents.len(), 1, "{contents:?}");
    let (path, content) = &contents[0];
    assert_eq!(path, &PathBuf::from("guide.md"));
    assert_eq!(content.as_deref().expect("readable"), staged);
}

#[tokio::test]
async fn each_file_is_named_from_the_top_level_and_read_as_named_whatever_diff_relative_says() {
    let repo = GitRepo::init().await;
    let root = repo.root();
    fs::create_dir(root.join("sub")).expect("subdirectory");
    fs::write(root.join("guide.md"), "# Top\n").expect("write");
    fs::write(root.join("sub").join("guide.md"), "# Sub\n").expect("write");
    run_in(root, &["add", "guide.md", "sub/guide.md"]).await;
    run_in(root, &["config", "--local", "diff.relative", "true"]).await;

    let contents = staged_contents(&root.join("sub"), files::is_markdown)
        .await
        .expect("staged_contents");

    let read: Vec<_> = contents
        .iter()
        .map(|(path, content)| (path.clone(), content.as_deref().expect("readable")))
        .collect();
    assert_eq!(
        read,
        [
            (PathBuf::from("guide.md"), "# Top\n"),
            (PathBuf::from("sub/guide.md"), "# Sub\n"),
        ]
    );
}
