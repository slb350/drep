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
pub(super) fn ruff_fixture(root: &std::path::Path) -> PathBuf {
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
