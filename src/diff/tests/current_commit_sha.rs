//! `current_commit_sha`: criteria 29-30.
//!
//! The two responses are deliberately distinct: a real SHA is a real gate
//! input (cache key), and `"unknown"` is the deliberate "I tried, I failed,
//! do not blame the user" outcome that is *never* surfaced as a diff query
//! failure.

use crate::diff::current_commit_sha;

use super::support::GitRepo;

#[tokio::test]
async fn returns_a_forty_char_hex_sha_in_a_repo_with_a_commit() {
    let repo = GitRepo::init().await;
    let root = repo.root();

    let sha = current_commit_sha(root).await;
    assert_eq!(sha.len(), 40, "expected a 40-character SHA, got {sha:?}");
    assert!(
        sha.chars().all(|c| c.is_ascii_hexdigit()),
        "expected a hex SHA, got {sha:?}"
    );
    assert!(
        !sha.chars().all(|c| c == '0'),
        "SHAs are never all-zero in practice"
    );
}

#[tokio::test]
async fn returns_the_unknown_literal_outside_a_git_repository() {
    // "Could not ask git" must NOT collapse into a different-looking
    // answer. `unknown` is what cache keys see; seeing anything else
    // here would mean the same code took a different path and could
    // distinguish the cases.
    let dir = tempfile::tempdir().expect("tempdir");

    let sha = current_commit_sha(dir.path()).await;
    assert_eq!(
        sha, "unknown",
        "expected the literal `unknown` outside a git repo, got {sha:?}"
    );
}
