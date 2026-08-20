//! Adding `drep.toml` to `.gitignore`, and the two states where doing so is
//! useless.

use crate::cli::init::gitignore::{Outcome, ensure};

/// A fresh git repository, with `.gitignore` seeded to `contents` when given.
fn repo(contents: Option<&str>) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    crate::test_support::git_init(dir.path());
    if let Some(body) = contents {
        std::fs::write(dir.path().join(".gitignore"), body).expect("write .gitignore");
    }
    dir
}

/// The repository's `.gitignore`, or `None` if there is none.
fn read(dir: &tempfile::TempDir) -> Option<String> {
    std::fs::read_to_string(dir.path().join(".gitignore")).ok()
}

/// The block `ensure` appends, without any leading separator.
fn block() -> String {
    "# drep's local config: provider and model choice, no secrets.\ndrep.toml\n".to_string()
}

#[tokio::test]
async fn a_repository_with_no_gitignore_gets_one() {
    let dir = repo(None);

    let outcome = ensure(dir.path()).await.expect("ensure");

    assert_eq!(outcome, Outcome::Created);
    // Exact, not `contains`: a leading blank line is invisible to a substring
    // check and is what an off-by-one in the separator logic produces.
    assert_eq!(read(&dir).expect("a .gitignore now exists"), block());
}

#[tokio::test]
async fn an_existing_gitignore_is_appended_to_after_one_blank_line() {
    let dir = repo(Some("target/\n*.log\n"));

    let outcome = ensure(dir.path()).await.expect("ensure");

    assert_eq!(outcome, Outcome::Added);
    assert_eq!(
        read(&dir).expect("still exists"),
        format!("target/\n*.log\n\n{}", block()),
        "the existing rules survive, separated by exactly one blank line"
    );
}

#[tokio::test]
async fn a_file_with_no_trailing_newline_is_repaired_before_appending() {
    // Otherwise the comment joins the last line and *both* rules stop working -
    // and the file that gets damaged is one drep did not write.
    let dir = repo(Some("target/"));

    ensure(dir.path()).await.expect("ensure");

    assert_eq!(
        read(&dir).expect("still exists"),
        format!("target/\n\n{}", block()),
        "the missing newline is supplied, then the separator"
    );
}

#[tokio::test]
async fn the_entry_is_annotated_with_what_put_it_there() {
    // A bare line in someone's .gitignore with no attribution is a small mystery
    // six months later.
    let dir = repo(None);

    ensure(dir.path()).await.expect("ensure");

    let body = read(&dir).expect("exists");
    assert!(
        body.lines()
            .any(|line| line.starts_with('#') && line.contains("drep")),
        "got {body:?}"
    );
}

#[tokio::test]
async fn an_already_ignored_path_is_left_alone() {
    let dir = repo(Some("drep.toml\n"));

    let outcome = ensure(dir.path()).await.expect("ensure");

    assert_eq!(outcome, Outcome::AlreadyIgnored);
    assert_eq!(
        read(&dir).expect("exists"),
        "drep.toml\n",
        "a second identical rule changes nothing and reads as a mistake"
    );
}

#[tokio::test]
async fn a_path_ignored_by_a_glob_is_recognised_as_ignored() {
    // The reason this asks git rather than comparing lines: no line-by-line
    // check would see that `*.toml` already covers `drep.toml`.
    let dir = repo(Some("*.toml\n"));

    let outcome = ensure(dir.path()).await.expect("ensure");

    assert_eq!(outcome, Outcome::AlreadyIgnored);
    assert_eq!(read(&dir).expect("exists"), "*.toml\n");
}

#[tokio::test]
async fn a_tracked_file_is_reported_rather_than_ignored() {
    // `.gitignore` has no effect on a file git already tracks. Appending would
    // look like it worked while `git status` kept showing the file, with nothing
    // to explain why.
    let dir = repo(None);
    std::fs::write(dir.path().join("drep.toml"), "[[llm]]\n").expect("write config");
    crate::test_support::git_add(dir.path(), "drep.toml");

    let outcome = ensure(dir.path()).await.expect("ensure");

    assert_eq!(outcome, Outcome::Tracked);
    assert!(
        read(&dir).is_none(),
        "nothing was written, because writing would have implied it worked"
    );
}

#[tokio::test]
async fn the_tracked_message_says_how_to_fix_it() {
    let message = Outcome::Tracked.message();

    assert!(
        message.contains("git rm --cached"),
        "the fix has to be in the message: {message}"
    );
}

#[test]
fn every_outcome_has_its_own_message() {
    // The four are reported to the user and mean different things; two sharing a
    // sentence would make the report useless for telling them apart.
    let messages = [
        Outcome::Added.message(),
        Outcome::Created.message(),
        Outcome::AlreadyIgnored.message(),
        Outcome::Tracked.message(),
    ];

    let mut unique = messages.to_vec();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), 4, "messages collide: {messages:?}");
}
