//! The deterministic layer in staged mode: a linter that would read
//! working-tree content the commit does not hold is refused rather than run,
//! and its files fail with `UncommittedChanges` naming what differs.

use std::path::PathBuf;

use super::deterministic::work_for;
use crate::analysis::result::FailureReason;
use crate::cli::check::deterministic;
use crate::test_support::write_executable;

/// A `ruff` fixture: the stub appends one line (its argv, joined) to
/// `argv.txt` per invocation, so a refusal is observable as the file never
/// appearing and a run as exactly one line.
///
/// Returns the stub's argv record. `pyproject.toml` opts the project into
/// ruff; without it the tool is `Skipped` and a refusal test would pass for
/// the wrong reason.
fn ruff_fixture(root: &std::path::Path) -> PathBuf {
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
    std::fs::write(root.join("pyproject.toml"), "").expect("pyproject");
    argv
}

/// Staged mode, criterion: a per-file tool whose file differs from the
/// committing index is not run - it would lint content the commit does not
/// hold - and the file fails with `UncommittedChanges` naming it.
#[tokio::test]
async fn a_tool_whose_file_is_uncommitted_does_not_run() {
    let dir = tempfile::tempdir().expect("tempdir");
    let argv = ruff_fixture(dir.path());
    let a = dir.path().join("a.py");
    std::fs::write(&a, "a = 1\n").expect("a.py");

    let mut work = work_for(std::slice::from_ref(&a));
    work.uncommitted = [PathBuf::from("a.py")].into_iter().collect();
    let (findings, failures, _compiled) = deterministic::run(&work, dir.path()).await;

    assert!(
        !argv.exists(),
        "ruff must not run on content the commit does not hold"
    );
    assert!(
        findings.is_empty(),
        "a refused tool contributes no findings, got {findings:?}"
    );
    match failures.get(&a) {
        Some(FailureReason::UncommittedChanges { tool, paths }) => {
            assert_eq!(tool, "ruff");
            assert_eq!(paths, &[PathBuf::from("a.py")]);
        }
        other => panic!("expected UncommittedChanges for {a:?}, got {other:?}"),
    }
}

/// A per-file tool also reads the config file its workspace holds, so an
/// uncommitted *config* refuses the run even when the linted file is clean.
#[tokio::test]
async fn a_tool_whose_config_file_is_uncommitted_does_not_run() {
    let dir = tempfile::tempdir().expect("tempdir");
    let argv = ruff_fixture(dir.path());
    let a = dir.path().join("a.py");
    std::fs::write(&a, "a = 1\n").expect("a.py");

    let mut work = work_for(std::slice::from_ref(&a));
    work.uncommitted = [PathBuf::from("pyproject.toml")].into_iter().collect();
    let (_findings, failures, _compiled) = deterministic::run(&work, dir.path()).await;

    assert!(
        !argv.exists(),
        "ruff must not run against a config the commit does not hold"
    );
    match failures.get(&a) {
        Some(FailureReason::UncommittedChanges { tool, paths }) => {
            assert_eq!(tool, "ruff");
            assert_eq!(paths, &[PathBuf::from("pyproject.toml")]);
        }
        other => panic!("expected UncommittedChanges for {a:?}, got {other:?}"),
    }
}

/// A tool that follows sources is refused by an uncommitted file of another
/// registered language too: cppcheck on a C++ file includes a header drep
/// calls C, and TypeScript imports JavaScript.
#[tokio::test]
async fn a_source_following_tool_refuses_an_uncommitted_file_of_another_language() {
    let dir = tempfile::tempdir().expect("tempdir");
    let argv = ruff_fixture(dir.path());
    let a = dir.path().join("a.py");
    std::fs::write(&a, "a = 1\n").expect("a.py");

    let mut work = work_for(std::slice::from_ref(&a));
    work.uncommitted = [PathBuf::from("setup.sh")].into_iter().collect();
    let (_findings, failures, _compiled) = deterministic::run(&work, dir.path()).await;

    assert!(
        !argv.exists(),
        "ruff must not run beside any uncommitted file"
    );
    match failures.get(&a) {
        Some(FailureReason::UncommittedChanges { tool, paths }) => {
            assert_eq!(tool, "ruff");
            assert_eq!(paths, &[PathBuf::from("setup.sh")]);
        }
        other => panic!("expected UncommittedChanges for {a:?}, got {other:?}"),
    }
}

/// A per-file tool that reads other sources of its language under its
/// workspace (ShellCheck follows `source`, eslint resolves imports, ruff
/// checks which modules and packages exist) is refused by an uncommitted
/// edit to one even when the batch file itself is clean, and the failure
/// names the helper, not the batch file.
#[tokio::test]
async fn a_per_file_tool_refuses_a_same_language_uncommitted_path_under_its_workspace() {
    let dir = tempfile::tempdir().expect("tempdir");
    let argv = ruff_fixture(dir.path());
    let a = dir.path().join("a.py");
    std::fs::write(&a, "a = 1\n").expect("a.py");

    let mut work = work_for(std::slice::from_ref(&a));
    work.uncommitted = [PathBuf::from("lib.py")].into_iter().collect();
    let (_findings, failures, _compiled) = deterministic::run(&work, dir.path()).await;

    assert!(
        !argv.exists(),
        "ruff must not run against a helper the commit does not hold"
    );
    match failures.get(&a) {
        Some(FailureReason::UncommittedChanges { tool, paths }) => {
            assert_eq!(tool, "ruff");
            assert_eq!(paths, &[PathBuf::from("lib.py")]);
        }
        other => panic!("expected UncommittedChanges for {a:?}, got {other:?}"),
    }
}

/// A tool that reads other sources is refused by an uncommitted file of no
/// registered language too: ShellCheck follows `source` into a helper with
/// any name, so an extensionless one can hold what the commit does not.
#[tokio::test]
async fn a_source_following_tool_refuses_an_uncommitted_file_of_no_registered_language() {
    let dir = tempfile::tempdir().expect("tempdir");
    let argv = ruff_fixture(dir.path());
    let a = dir.path().join("a.py");
    std::fs::write(&a, "a = 1\n").expect("a.py");

    let mut work = work_for(std::slice::from_ref(&a));
    work.uncommitted = [PathBuf::from("lib/common")].into_iter().collect();
    let (_findings, failures, _compiled) = deterministic::run(&work, dir.path()).await;

    assert!(
        !argv.exists(),
        "a tool that follows sources must not run beside an unclassified uncommitted file"
    );
    match failures.get(&a) {
        Some(FailureReason::UncommittedChanges { tool, paths }) => {
            assert_eq!(tool, "ruff");
            assert_eq!(paths, &[PathBuf::from("lib/common")]);
        }
        other => panic!("expected UncommittedChanges for {a:?}, got {other:?}"),
    }
}

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

/// A whole-project tool (`accepts_files: false`) is invoked bare from its
/// workspace and reads all of it, so any differing path under the workspace
/// refuses the run.
#[tokio::test]
async fn a_whole_project_tool_refuses_any_uncommitted_path_under_its_workspace() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let bin = root.join("node_modules/.bin/tsc");
    std::fs::create_dir_all(bin.parent().unwrap()).expect("bin dir");
    let counter = root.join("counter.txt");
    write_executable(
        &bin,
        format!(
            "#!/bin/sh\nprintf '%s\\n' called >> {}\n",
            counter.display()
        ),
    );
    let member = root.join("apps/web");
    std::fs::create_dir_all(member.join("src")).expect("member dirs");
    std::fs::write(member.join("tsconfig.json"), "{}\n").expect("tsconfig");
    let file = member.join("src/app.ts");
    std::fs::write(&file, "const x: number = 1;\n").expect("source");

    let mut work = work_for(std::slice::from_ref(&file));
    work.uncommitted = [PathBuf::from("apps/web/tsconfig.json")]
        .into_iter()
        .collect();
    let (_findings, failures, _compiled) = deterministic::run(&work, root).await;
    assert!(
        !counter.exists(),
        "tsc must not run while its workspace differs from the commit"
    );
    match failures.get(&file) {
        Some(FailureReason::UncommittedChanges { tool, paths }) => {
            assert_eq!(tool, "tsc");
            assert_eq!(paths, &[PathBuf::from("apps/web/tsconfig.json")]);
        }
        other => panic!("expected UncommittedChanges for {file:?}, got {other:?}"),
    }
}

/// A whole-project tool reads beyond its workspace too, through a path
/// dependency or a relative import, so an uncommitted file outside the
/// workspace refuses it.
#[tokio::test]
async fn a_whole_project_tool_refuses_an_uncommitted_file_outside_its_workspace() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let bin = root.join("node_modules/.bin/tsc");
    std::fs::create_dir_all(bin.parent().unwrap()).expect("bin dir");
    let counter = root.join("counter.txt");
    write_executable(
        &bin,
        format!(
            "#!/bin/sh\nprintf '%s\\n' called >> {}\n",
            counter.display()
        ),
    );
    let member = root.join("apps/web");
    std::fs::create_dir_all(member.join("src")).expect("member dirs");
    std::fs::write(member.join("tsconfig.json"), "{}\n").expect("tsconfig");
    let file = member.join("src/app.ts");
    std::fs::write(&file, "const x: number = 1;\n").expect("source");

    let mut work = work_for(std::slice::from_ref(&file));
    work.uncommitted = [PathBuf::from("shared/lib.ts")].into_iter().collect();
    let (_findings, failures, _compiled) = deterministic::run(&work, root).await;
    assert!(
        !counter.exists(),
        "tsc must not run while a TypeScript file it can import differs"
    );
    match failures.get(&file) {
        Some(FailureReason::UncommittedChanges { tool, paths }) => {
            assert_eq!(tool, "tsc");
            assert_eq!(paths, &[PathBuf::from("shared/lib.ts")]);
        }
        other => panic!("expected UncommittedChanges for {file:?}, got {other:?}"),
    }
}

/// A whole-project tool reads every file under its workspace, tracked or
/// not: tsc compiles an untracked `helper.ts` an import names, so an
/// untracked file under the workspace refuses the run exactly as a modified
/// tracked one does.
#[tokio::test]
async fn a_whole_project_tool_refuses_an_untracked_file_under_its_workspace() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let bin = root.join("node_modules/.bin/tsc");
    std::fs::create_dir_all(bin.parent().unwrap()).expect("bin dir");
    let counter = root.join("counter.txt");
    write_executable(
        &bin,
        format!(
            "#!/bin/sh\nprintf '%s\\n' called >> {}\n",
            counter.display()
        ),
    );
    let member = root.join("apps/web");
    std::fs::create_dir_all(member.join("src")).expect("member dirs");
    std::fs::write(member.join("tsconfig.json"), "{}\n").expect("tsconfig");
    let file = member.join("src/app.ts");
    std::fs::write(&file, "const x: number = 1;\n").expect("source");

    let mut work = work_for(std::slice::from_ref(&file));
    // `ls-files --others` names this file: it exists on disk but the commit
    // has never heard of it.
    work.uncommitted = [PathBuf::from("apps/web/src/helper.ts")]
        .into_iter()
        .collect();
    let (_findings, failures, _compiled) = deterministic::run(&work, root).await;

    assert!(
        !counter.exists(),
        "tsc must not run while its workspace holds a file the commit does not"
    );
    match failures.get(&file) {
        Some(FailureReason::UncommittedChanges { tool, paths }) => {
            assert_eq!(tool, "tsc");
            assert_eq!(paths, &[PathBuf::from("apps/web/src/helper.ts")]);
        }
        other => panic!("expected UncommittedChanges for {file:?}, got {other:?}"),
    }
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
