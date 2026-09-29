//! The deterministic layer in staged mode, for what configures a tool and
//! the files a tool that reads only its own opens: a config marker the
//! working tree does not hold, however it is reached, fails the files it
//! configures in the commit.

use std::path::PathBuf;

use super::deterministic::work_for;
use super::deterministic_staged::ruff_fixture;
use crate::analysis::result::FailureReason;
use crate::cli::check::deterministic;
use crate::test_support::write_executable;

/// A project opted into hadolint, a tool that reads nothing but the files it
/// is given (`reads_other_sources: false`), by a `.hadolint.yaml` at `root`.
/// Returns a Dockerfile written at `name` beneath it.
fn hadolint_fixture(root: &std::path::Path, name: &str) -> PathBuf {
    std::fs::write(root.join(".hadolint.yaml"), "").expect("hadolint config");
    let file = root.join(name);
    std::fs::create_dir_all(file.parent().unwrap()).expect("dockerfile dir");
    std::fs::write(&file, "FROM scratch\n").expect("dockerfile");
    file
}

/// Runs the deterministic layer over `file` with `uncommitted` differing and
/// returns the paths hadolint's refusal names, `None` when it was not
/// refused. hadolint need not be installed: a refused tool never starts, and
/// one that is not refused fails as unavailable or runs.
async fn hadolint_refusal(
    root: &std::path::Path,
    file: &std::path::Path,
    uncommitted: &[&str],
) -> Option<Vec<PathBuf>> {
    let mut work = work_for(&[file.to_path_buf()]);
    work.uncommitted = uncommitted.iter().map(PathBuf::from).collect();
    let (_findings, failures, _compiled) = deterministic::run(&work, root).await;
    match failures.get(file) {
        Some(FailureReason::UncommittedChanges { tool, paths }) => {
            assert_eq!(tool, "hadolint");
            Some(paths.clone())
        }
        _ => None,
    }
}

/// Even a tool that reads only its files reads those files and its
/// configuration: an uncommitted one of either refuses it.
#[tokio::test]
async fn a_tool_reading_only_its_files_refuses_its_own_file_and_config() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = hadolint_fixture(dir.path(), "Dockerfile");

    assert_eq!(
        hadolint_refusal(dir.path(), &file, &["Dockerfile"]).await,
        Some(vec![PathBuf::from("Dockerfile")])
    );
    assert_eq!(
        hadolint_refusal(dir.path(), &file, &[".hadolint.yaml"]).await,
        Some(vec![PathBuf::from(".hadolint.yaml")])
    );
}

/// A batch file that is a symlink, or a marker in a symlinked directory,
/// reads its target, and git names the target when it differs, not the link:
/// an uncommitted target refuses even a tool that reads only its files.
#[tokio::test]
async fn a_symlinked_file_or_marker_refuses_the_tool_when_its_target_differs() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    hadolint_fixture(root, "real/Dockerfile");
    std::fs::create_dir_all(root.join("svc")).expect("svc dir");
    let link = root.join("svc/Dockerfile");
    std::os::unix::fs::symlink("../real/Dockerfile", &link).expect("dockerfile link");
    assert_eq!(
        hadolint_refusal(root, &link, &["real/Dockerfile"]).await,
        Some(vec![PathBuf::from("real/Dockerfile")])
    );

    // A marker reached through a symlinked directory is its target too, which
    // no link of the marker's own names.
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    hadolint_fixture(root, "real/Dockerfile");
    std::fs::write(root.join("real/.hadolint.yaml"), "").expect("nearer config");
    std::os::unix::fs::symlink("real", root.join("svc")).expect("directory link");
    let file = root.join("svc/Dockerfile");
    assert_eq!(
        hadolint_refusal(root, &file, &["real/.hadolint.yaml"]).await,
        Some(vec![PathBuf::from("real/.hadolint.yaml")])
    );
}

/// A marker that is a symlink whose target the working tree no longer holds
/// is missing on disk, and git reports the deleted target, not the unchanged
/// link: the target still names the commit's configuration, whether the link
/// was the file's only marker or a nearer one than the marker that
/// configured it.
#[tokio::test]
async fn a_marker_link_to_a_deleted_target_refuses_the_tool() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let file = hadolint_fixture(root, "Dockerfile");
    std::fs::remove_file(root.join(".hadolint.yaml")).expect("remove the marker");
    std::os::unix::fs::symlink("shared/hadolint.yaml", root.join(".hadolint.yaml"))
        .expect("dangling marker link");
    assert_eq!(
        hadolint_refusal(root, &file, &["shared/hadolint.yaml"]).await,
        Some(vec![PathBuf::from("shared/hadolint.yaml")])
    );

    // A chain of links is followed to its end.
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let file = hadolint_fixture(root, "Dockerfile");
    std::fs::remove_file(root.join(".hadolint.yaml")).expect("remove the marker");
    std::os::unix::fs::symlink("chain.yaml", root.join(".hadolint.yaml")).expect("first link");
    std::os::unix::fs::symlink("shared/hadolint.yaml", root.join("chain.yaml"))
        .expect("second, dangling link");
    assert_eq!(
        hadolint_refusal(root, &file, &["shared/hadolint.yaml"]).await,
        Some(vec![PathBuf::from("shared/hadolint.yaml")])
    );

    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let file = hadolint_fixture(root, "pkg/Dockerfile");
    std::os::unix::fs::symlink("../shared/pkg.yaml", root.join("pkg/.hadolint.yaml"))
        .expect("dangling nearer marker link");
    assert_eq!(
        hadolint_refusal(root, &file, &["shared/pkg.yaml"]).await,
        Some(vec![PathBuf::from("shared/pkg.yaml")])
    );
}

/// The paths an `UncommittedChanges` failure of `file` names when
/// `index_only` is what a sparse checkout left out, `None` when it has none.
async fn index_only_refusal(
    root: &std::path::Path,
    file: &std::path::Path,
    index_only: &[&str],
) -> Option<Vec<PathBuf>> {
    let mut work = work_for(&[file.to_path_buf()]);
    work.index_only = index_only.iter().map(PathBuf::from).collect();
    let (_findings, failures, _compiled) = deterministic::run(&work, root).await;
    match failures.get(file) {
        Some(FailureReason::UncommittedChanges { paths, .. }) => Some(paths.clone()),
        _ => None,
    }
}

/// A marker a sparse checkout left out of the working tree (`skip-worktree`,
/// index-only) is the commit's configuration all the same, whether the tool
/// found no other or a farther one; an index-only file that is no marker
/// refuses nothing, since no tool can read it.
#[tokio::test]
async fn a_marker_a_sparse_checkout_left_out_refuses_the_tool() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let file = hadolint_fixture(root, "pkg/Dockerfile");
    assert_eq!(
        index_only_refusal(root, &file, &["pkg/.hadolint.yaml"]).await,
        Some(vec![PathBuf::from("pkg/.hadolint.yaml")])
    );
    std::fs::remove_file(root.join(".hadolint.yaml")).expect("remove the root marker");
    assert_eq!(
        index_only_refusal(root, &file, &[".hadolint.yaml"]).await,
        Some(vec![PathBuf::from(".hadolint.yaml")])
    );

    let dir = tempfile::tempdir().expect("tempdir");
    let argv = ruff_fixture(dir.path());
    let a = dir.path().join("a.py");
    std::fs::write(&a, "a = 1\n").expect("a.py");
    let mut work = work_for(std::slice::from_ref(&a));
    work.index_only = [PathBuf::from("lib.py")].into_iter().collect();
    let (_findings, failures, _compiled) = deterministic::run(&work, dir.path()).await;
    assert!(
        failures.is_empty(),
        "no tool reads an index-only file, got {failures:?}"
    );
    assert!(argv.exists(), "ruff runs beside it");
}

/// A nearer config marker the working tree no longer holds, in any directory
/// between the file and the one that configured it on disk, is the commit's
/// configuration, so an uncommitted one refuses the tool rather than letting
/// it lint by the farther.
#[tokio::test]
async fn a_nearer_marker_missing_from_the_working_tree_refuses_the_tool() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = hadolint_fixture(dir.path(), "pkg/sub/Dockerfile");

    assert_eq!(
        hadolint_refusal(dir.path(), &file, &["pkg/.hadolint.yaml"]).await,
        Some(vec![PathBuf::from("pkg/.hadolint.yaml")])
    );
}

/// A per-file tool that reads nothing but the files it is given
/// (`reads_other_sources: false`, as hadolint is) is not refused by an
/// uncommitted file of its language: nothing it reads differs from the
/// commit.
#[tokio::test]
async fn a_tool_reading_only_its_files_runs_beside_an_uncommitted_file_of_its_language() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = hadolint_fixture(dir.path(), "Dockerfile");

    assert_eq!(
        hadolint_refusal(dir.path(), &file, &["other/Dockerfile"]).await,
        None
    );
}

/// A staged file whose tool's config marker is in the committing index but
/// missing from the working tree finds no configured workspace on disk - and
/// that must fail the file naming the marker, not silently skip the tool:
/// the commit opted into ruff even though the working tree no longer shows
/// it.
#[tokio::test]
async fn a_config_marker_missing_from_the_working_tree_fails_the_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let bin = root.join("venv/bin/ruff");
    std::fs::create_dir_all(bin.parent().unwrap()).expect("bin dir");
    let argv = root.join("argv.txt");
    write_executable(
        &bin,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> {}\nprintf '%s' '[]'\n",
            argv.display()
        ),
    );
    // Deliberately no `pyproject.toml` on disk: the committing index holds
    // it, the working tree does not.
    let a = root.join("a.py");
    std::fs::write(&a, "a = 1\n").expect("a.py");

    let mut work = work_for(std::slice::from_ref(&a));
    work.uncommitted = [PathBuf::from("pyproject.toml")].into_iter().collect();
    let (_findings, failures, _compiled) = deterministic::run(&work, root).await;

    assert!(
        !argv.exists(),
        "ruff never ran, so this is not a run refusal"
    );
    match failures.get(&a) {
        Some(FailureReason::UncommittedChanges { tool, paths }) => {
            assert_eq!(tool, "ruff");
            assert_eq!(paths, &[PathBuf::from("pyproject.toml")]);
        }
        other => panic!("expected UncommittedChanges for {a:?}, got {other:?}"),
    }
}

/// A marker can name a nested path, as Checkstyle's
/// `config/checkstyle/checkstyle.xml` does: one the working tree no longer
/// holds fails the file it would have configured, as a marker at the root
/// itself would.
#[tokio::test]
async fn a_nested_config_marker_missing_from_the_working_tree_fails_the_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let a = root.join("A.java");
    std::fs::write(&a, "class A {}\n").expect("A.java");

    let mut work = work_for(std::slice::from_ref(&a));
    work.uncommitted = [PathBuf::from("config/checkstyle/checkstyle.xml")]
        .into_iter()
        .collect();
    let (_findings, failures, _compiled) = deterministic::run(&work, root).await;

    match failures.get(&a) {
        Some(FailureReason::UncommittedChanges { tool, paths }) => {
            assert_eq!(tool, "checkstyle");
            assert_eq!(paths, &[PathBuf::from("config/checkstyle/checkstyle.xml")]);
        }
        other => panic!("expected UncommittedChanges for {a:?}, got {other:?}"),
    }
}

/// An uncommitted marker configures only the files at or below its own
/// directory: one in a sibling directory says nothing about a file outside
/// it, whose unconfigured tool stays skipped as it is in every other mode.
#[tokio::test]
async fn an_uncommitted_marker_beside_the_files_directory_does_not_fail_it() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::create_dir_all(root.join("pkg")).expect("pkg dir");
    let a = root.join("pkg/a.py");
    std::fs::write(&a, "a = 1\n").expect("a.py");

    let mut work = work_for(std::slice::from_ref(&a));
    work.uncommitted = [PathBuf::from("other/pyproject.toml")]
        .into_iter()
        .collect();
    let (_findings, failures, _compiled) = deterministic::run(&work, root).await;

    assert!(
        failures.is_empty(),
        "a marker outside the file's ancestry must not fail it, got {failures:?}"
    );
}
