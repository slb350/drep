//! `uncommitted_paths`: the files a linter reading the working tree would see in a form the commit does not hold.

use std::fs;
use std::path::{Path, PathBuf};

use crate::diff::{Uncommitted, uncommitted_paths};

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

/// Edited-after-staging, deleted-from-the-working-tree and untracked files
/// all differ from what the commit holds; a fully staged file and an ignored
/// untracked file do not.
#[tokio::test]
async fn lists_tracked_differences_and_untracked_files_but_not_ignored_ones() {
    let repo = GitRepo::init().await;
    let root = repo.root();

    fs::write(root.join("edited.py"), "a = 1\n").expect("write edited");
    fs::write(root.join("deleted.py"), "b = 1\n").expect("write deleted");
    fs::write(root.join("staged.py"), "c = 1\n").expect("write staged");
    run_in(root, &["add", "edited.py", "deleted.py", "staged.py"]).await;

    fs::write(root.join("edited.py"), "a = 2\n").expect("edit after staging");
    fs::remove_file(root.join("deleted.py")).expect("delete from the working tree");
    fs::write(root.join("untracked.py"), "d = 1\n").expect("write untracked");
    fs::write(root.join(".gitignore"), "ignored.py\n").expect("write gitignore");
    fs::write(root.join("ignored.py"), "e = 1\n").expect("write ignored");
    // The `.gitignore` is itself staged so only `ignored.py` tests exclusion.
    run_in(root, &["add", ".gitignore"]).await;

    let paths = uncommitted_paths(root)
        .await
        .expect("uncommitted_paths")
        .differing;
    assert_eq!(
        paths,
        vec![
            PathBuf::from("deleted.py"),
            PathBuf::from("edited.py"),
            PathBuf::from("untracked.py"),
        ],
        "edited, deleted and untracked-not-ignored files, nothing else"
    );
}

/// A name is listed byte for byte: one that begins with a space keeps it,
/// tracked or untracked, where a trimmed listing named another file.
#[tokio::test]
async fn keeps_a_leading_space_in_a_name() {
    let repo = GitRepo::init().await;
    let root = repo.root();
    fs::write(root.join(" edited.lua"), "local a = 1\n").expect("write edited");
    run_in(root, &["add", " edited.lua"]).await;
    fs::write(root.join(" edited.lua"), "local a = 2\n").expect("edit after staging");
    fs::write(root.join(" untracked.lua"), "local b = 1\n").expect("write untracked");

    let paths = uncommitted_paths(root)
        .await
        .expect("uncommitted_paths")
        .differing;
    assert_eq!(
        paths,
        vec![
            PathBuf::from(" edited.lua"),
            PathBuf::from(" untracked.lua")
        ]
    );
}

/// A dirty submodule is listed even where configuration tells `git diff` to
/// ignore submodules: a staged file can build against its working-tree
/// content.
#[tokio::test]
async fn lists_a_dirty_submodule_configuration_ignores() {
    let library = GitRepo::init().await;
    fs::write(library.root().join("lib.rs"), "pub fn a() {}\n").expect("write lib");
    library.commit_all("library").await;
    let repo = GitRepo::init().await;
    let root = repo.root();
    let source = library.root().to_str().expect("utf-8 path");
    run_in(
        root,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            source,
            "shared",
        ],
    )
    .await;
    repo.commit_all("add the submodule").await;
    fs::write(root.join("shared/lib.rs"), "pub fn b() {}\n").expect("edit the submodule");
    run_in(root, &["config", "diff.ignoreSubmodules", "all"]).await;

    let paths = uncommitted_paths(root)
        .await
        .expect("uncommitted_paths")
        .differing;
    assert_eq!(paths, vec![PathBuf::from("shared")]);
}

/// An edit to a file whose index entry tells git not to compare it
/// (`assume-unchanged`, `skip-worktree`) is listed: `git diff` hides both. A
/// flagged file left as the index holds it is not, nor is a `skip-worktree`
/// file absent from the working tree, as a sparse checkout leaves it; an
/// assumed-unchanged file deleted from the working tree, or with a directory
/// in its place, is. A flagged link is compared by its text, and a link in
/// place of a flagged file differs whatever its text. The absent
/// `skip-worktree` file is index-only.
#[tokio::test]
async fn lists_edits_index_flags_hide() {
    let repo = GitRepo::init().await;
    let root = repo.root();
    for name in [
        "assumed.ts",
        "skipped.ts",
        "untouched.ts",
        "sparse.ts",
        "deleted.ts",
        "replaced.ts",
    ] {
        fs::write(root.join(name), "export const a = 1;\n").expect("write");
    }
    // A file whose content happens to spell a link target, so only its mode
    // tells it from the link that later takes its place.
    fs::write(root.join("retyped.ts"), "untouched.ts").expect("write retyped");
    fs::write(root.join("private.ts"), "export const p = 1;\n").expect("write private");
    std::os::unix::fs::symlink("untouched.ts", root.join("kept.ts")).expect("kept link");
    std::os::unix::fs::symlink("untouched.ts", root.join("moved.ts")).expect("moved link");
    repo.commit_all("track them").await;
    for (flag, name) in [
        ("--assume-unchanged", "kept.ts"),
        ("--assume-unchanged", "retyped.ts"),
        ("--skip-worktree", "moved.ts"),
        ("--assume-unchanged", "assumed.ts"),
        ("--skip-worktree", "skipped.ts"),
        ("--assume-unchanged", "untouched.ts"),
        ("--skip-worktree", "sparse.ts"),
        ("--assume-unchanged", "deleted.ts"),
        ("--assume-unchanged", "replaced.ts"),
    ] {
        run_in(root, &["update-index", flag, name]).await;
    }
    fs::write(root.join("assumed.ts"), "export const a = 2;\n").expect("edit assumed");
    fs::write(root.join("skipped.ts"), "export const a = 3;\n").expect("edit skipped");
    fs::remove_file(root.join("sparse.ts")).expect("leave sparse out");
    fs::remove_file(root.join("deleted.ts")).expect("delete assumed");
    fs::remove_file(root.join("replaced.ts")).expect("remove replaced");
    fs::create_dir(root.join("replaced.ts")).expect("a directory in its place");
    fs::remove_file(root.join("moved.ts")).expect("remove moved link");
    std::os::unix::fs::symlink("assumed.ts", root.join("moved.ts")).expect("retarget moved link");
    fs::remove_file(root.join("retyped.ts")).expect("remove retyped");
    std::os::unix::fs::symlink("untouched.ts", root.join("retyped.ts"))
        .expect("a link in its place");

    // An unflagged file is `git diff`'s to judge and is never hashed, so one
    // that cannot be read does not fail the listing.
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(root.join("private.ts"), fs::Permissions::from_mode(0o000))
        .expect("make private unreadable");
    let unreadable = fs::File::open(root.join("private.ts")).is_err();

    let mut listed = uncommitted_paths(root).await.expect("uncommitted_paths");
    fs::set_permissions(root.join("private.ts"), fs::Permissions::from_mode(0o644))
        .expect("restore private");
    let private = PathBuf::from("private.ts");
    assert_eq!(
        listed.differing.contains(&private),
        unreadable,
        "git diff reports it"
    );
    listed.differing.retain(|path| *path != private);
    assert_eq!(
        listed.differing,
        vec![
            PathBuf::from("assumed.ts"),
            PathBuf::from("deleted.ts"),
            PathBuf::from("moved.ts"),
            PathBuf::from("replaced.ts"),
            PathBuf::from("retyped.ts"),
            PathBuf::from("skipped.ts"),
        ],
        "a flagged link is compared by its text: kept.ts still names its blob"
    );
    assert_eq!(listed.index_only, vec![PathBuf::from("sparse.ts")]);
}

/// A flagged submodule is compared with the commit its index entry records:
/// left clean, uninitialized or not checked out it is not listed; checked out
/// at another commit, edited, or replaced by a file it is, though `git diff`
/// trusts the flag and reports none of them.
#[tokio::test]
async fn compares_a_flagged_submodule_with_its_recorded_commit() {
    let library = GitRepo::init().await;
    fs::write(library.root().join("lib.rs"), "pub fn a() {}\n").expect("write lib");
    library.commit_all("first").await;
    fs::write(library.root().join("lib.rs"), "pub fn b() {}\n").expect("write lib again");
    library.commit_all("second").await;
    let repo = GitRepo::init().await;
    let root = repo.root();
    let source = library.root().to_str().expect("utf-8 path");
    run_in(
        root,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            source,
            "shared",
        ],
    )
    .await;
    repo.commit_all("add the submodule").await;
    run_in(root, &["update-index", "--assume-unchanged", "shared"]).await;
    let listed = || async {
        uncommitted_paths(root)
            .await
            .expect("uncommitted_paths")
            .differing
    };

    assert_eq!(
        listed().await,
        Vec::<PathBuf>::new(),
        "a clean flagged submodule"
    );
    run_in(&root.join("shared"), &["checkout", "-q", "HEAD~1"]).await;
    assert_eq!(
        listed().await,
        vec![PathBuf::from("shared")],
        "another commit"
    );
    run_in(&root.join("shared"), &["checkout", "-q", "-"]).await;
    fs::write(root.join("shared/lib.rs"), "pub fn c() {}\n").expect("edit the submodule");
    assert_eq!(
        listed().await,
        vec![PathBuf::from("shared")],
        "changes of its own"
    );

    // One never initialized, or not checked out, holds nothing a tool reads;
    // a file in its place is not the submodule.
    run_in(root, &["submodule", "deinit", "-q", "-f", "shared"]).await;
    assert_eq!(
        listed().await,
        Vec::<PathBuf>::new(),
        "an uninitialized submodule"
    );
    fs::remove_dir(root.join("shared")).expect("remove the empty checkout");
    assert_eq!(
        listed().await,
        Vec::<PathBuf>::new(),
        "a submodule not checked out"
    );
    fs::write(root.join("shared"), "not a submodule\n").expect("a file in its place");
    assert_eq!(
        listed().await,
        vec![PathBuf::from("shared")],
        "a file in its place"
    );
}

/// A clean filter rewrites what is committed and `git diff` compares the
/// rewritten content, so a file whose raw bytes differ from what checking
/// out its blob writes is listed though git reports it unchanged; one that
/// matches is not, nor is a link or a file whose only conversion is no filter.
#[tokio::test]
async fn lists_an_edit_a_clean_filter_hides() {
    let repo = GitRepo::init().await;
    let root = repo.root();
    run_in(
        root,
        &["config", "filter.strip.clean", "grep -v noqa || true"],
    )
    .await;
    run_in(root, &["config", "filter.strip.smudge", "cat"]).await;
    // A conversion that is no filter, and a link the filter's pattern names,
    // are left to `git diff`, whatever checking them out would write.
    fs::write(
        root.join(".gitattributes"),
        "*.py filter=strip\n*.txt eol=crlf\n",
    )
    .expect("write attributes");
    for name in ["edited.py", "kept.py", "lf.txt"] {
        fs::write(root.join(name), "a = 1\n").expect("write");
    }
    std::os::unix::fs::symlink("kept.py", root.join("link.py")).expect("link under the filter");
    repo.commit_all("track them").await;
    fs::write(root.join("edited.py"), "a = 1\nimport os  # noqa\n")
        .expect("edit behind the filter");

    let paths = uncommitted_paths(root)
        .await
        .expect("uncommitted_paths")
        .differing;
    assert_eq!(paths, vec![PathBuf::from("edited.py")]);
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
        uncommitted_paths(root).await.expect("ordinary index") == Uncommitted::default(),
        "the working tree matches the ordinary index"
    );

    let output = std::process::Command::new(std::env::current_exe().expect("this test binary"))
        .args([
            "diff::tests::uncommitted::under_a_hook_names_the_alternate_index",
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
    let paths = uncommitted_paths(Path::new(&root))
        .await
        .expect("uncommitted_paths")
        .differing;
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

    let paths = uncommitted_paths(&root.join("sub"))
        .await
        .expect("uncommitted_paths")
        .differing;
    assert_eq!(paths, vec![PathBuf::from("../top.py")]);
}

/// Run from a subdirectory, an untracked file above it is named from there
/// too: `ls-files` limits a bare listing to the working directory, so the
/// query selects the whole tree and the prefix conversion does the rest.
#[tokio::test]
async fn names_an_untracked_file_above_the_subdirectory() {
    let repo = GitRepo::init().await;
    let root = repo.root();
    fs::write(root.join("helper.py"), "a = 1\n").expect("write untracked");
    fs::create_dir_all(root.join("sub")).expect("subdirectory");

    let paths = uncommitted_paths(&root.join("sub"))
        .await
        .expect("uncommitted_paths")
        .differing;
    assert_eq!(paths, vec![PathBuf::from("../helper.py")]);
}
