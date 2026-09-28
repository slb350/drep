//! What `git diff` prints under the configuration drep inherits.
//!
//! drep parses git's output, and git formats that output from the user's
//! configuration: path quoting, prefixes, colour, an external diff program and
//! blank-context suppression all change the text. A shape the parser does not
//! recognise yields no hunk and no name, and a file with no hunk is a file the
//! gate reports as clean without reviewing it. These tests use git itself as
//! the oracle for both halves: the names it quotes, and the settings that
//! reshape a patch.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use crate::diff::hunks::Hunk;
use crate::diff::{hunks_between, staged_files, staged_hunks};
use crate::files;

use super::support::{GitRepo, run_in};

/// The distinct paths a set of hunks names.
fn hunk_paths(hunks: &[Hunk]) -> BTreeSet<PathBuf> {
    hunks.iter().map(|hunk| hunk.file_path.clone()).collect()
}

/// Every byte class git quotes, in a name drep analyzes.
///
/// A space is not quoted but earns the header a trailing tab; the rest are
/// C-quoted with the `b/` prefix inside the quotes. `plain.rs` is the control:
/// it is the only name the gate reviewed before quoted paths were decoded.
#[tokio::test]
async fn every_name_git_quotes_is_staged_under_its_own_path() {
    let repo = GitRepo::init().await;
    let root = repo.root();
    let names = [
        "plain.rs",
        "a b.rs",
        "caf\u{e9}.rs",
        "q\"uote.rs",
        "back\\slash.rs",
        "tab\there.rs",
        "new\nline.rs",
    ];
    for name in names {
        fs::write(root.join(name), "fn main() {}\n").expect("write");
    }
    run_in(root, &["add", "--all"]).await;
    let expected: BTreeSet<PathBuf> = names.iter().map(PathBuf::from).collect();

    let listed = staged_files(root, files::is_scan_target)
        .await
        .expect("staged_files");
    assert_eq!(
        listed.iter().cloned().collect::<BTreeSet<_>>(),
        expected,
        "--name-only quotes these names, got {listed:?}"
    );
    assert_eq!(listed.len(), names.len(), "each name once, got {listed:?}");

    let hunks = staged_hunks(root, files::is_scan_target)
        .await
        .expect("staged_hunks");
    assert_eq!(hunk_paths(&hunks), expected, "got {hunks:?}");
}

/// A name that is not UTF-8 reaches drep as the bytes it has on disk.
///
/// `core.quotePath=false` would make git print those bytes raw, where the
/// lossy conversion of git's stdout replaces them; drep pins the setting on so
/// they arrive as octal escapes it decodes exactly. Linux only, because APFS
/// refuses a name that is not UTF-8.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn a_name_that_is_not_utf8_keeps_its_bytes_whatever_core_quote_path_says() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;

    let repo = GitRepo::init().await;
    let root = repo.root();
    run_in(root, &["config", "core.quotePath", "false"]).await;
    let name = Path::new(OsStr::from_bytes(b"caf\xe9.rs"));
    fs::write(root.join(name), "fn main() {}\n").expect("write");
    run_in(root, &["add", "--all"]).await;

    let listed = staged_files(root, files::is_scan_target)
        .await
        .expect("staged_files");
    assert_eq!(listed, vec![name.to_path_buf()]);
    let hunks = staged_hunks(root, files::is_scan_target)
        .await
        .expect("staged_hunks");
    assert_eq!(hunk_paths(&hunks), BTreeSet::from([name.to_path_buf()]));
}

/// The change every configuration case makes: line 3 of `m.rs`, after a blank
/// context line, so a suppressed blank line shifts its number.
const BEFORE: &str = "fn a() {}\n\nfn b() {}\n";
const AFTER: &str = "fn a() {}\n\nfn c() {}\n";

/// How `hunks` differs from the one change to `m.rs` at its true line number,
/// or `None` when it is exactly that change.
fn mismatch(hunks: &[Hunk], setting: &str, query: &str) -> Option<String> {
    let added: Vec<(Option<u32>, &str)> = hunks
        .iter()
        .flat_map(Hunk::numbered_lines)
        .filter(|(_, line)| line.marker() == '+')
        .map(|(number, line)| (number, line.content()))
        .collect();
    let expected = vec![(Some(3), "fn c() {}")];
    (hunk_paths(hunks) != BTreeSet::from([PathBuf::from("m.rs")]) || added != expected)
        .then(|| format!("{query} under {setting}: {hunks:?}"))
}

/// Configuration that reshapes `git diff` output must not hide a change.
///
/// Each setting, left to apply, made every changed file vanish from both the
/// staged hook and the pushed range - `diff.suppressBlankEmpty` instead
/// misnumbered every line after a blank one. The external program prints what
/// a tool such as difftastic would: something that is not a unified diff.
#[tokio::test]
async fn user_diff_configuration_cannot_hide_a_change() {
    let tools = tempfile::tempdir().expect("tempdir");
    let external = tools.path().join("external-diff");
    crate::test_support::write_executable(&external, "#!/bin/sh\necho \"external: $1\"\n");
    let external = external.to_str().expect("utf-8 tempdir").to_owned();

    let mut hidden = Vec::new();
    for (key, value) in [
        ("diff.noprefix", "true"),
        ("diff.mnemonicPrefix", "true"),
        ("color.ui", "always"),
        ("diff.external", external.as_str()),
        ("diff.suppressBlankEmpty", "true"),
    ] {
        let setting = format!("{key}={value}");
        let repo = GitRepo::init().await;
        let root = repo.root();
        fs::write(root.join("m.rs"), BEFORE).expect("write");
        repo.commit_all("seed").await;
        repo.create_branch("feature").await;
        repo.checkout("feature").await;
        run_in(root, &["config", key, value]).await;
        fs::write(root.join("m.rs"), AFTER).expect("write");
        run_in(root, &["add", "m.rs"]).await;

        let staged = staged_hunks(root, files::is_scan_target)
            .await
            .expect("staged_hunks");
        hidden.extend(mismatch(&staged, &setting, "staged_hunks"));

        repo.commit_all("change").await;
        let pushed = hunks_between(root, "main", Some("feature"), files::is_scan_target)
            .await
            .expect("hunks_between");
        hidden.extend(mismatch(&pushed, &setting, "hunks_between"));
    }
    assert!(hidden.is_empty(), "{}", hidden.join("\n"));
}
