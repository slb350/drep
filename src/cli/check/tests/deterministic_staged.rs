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

/// A clean file's per-file tool runs exactly as before when the only
/// uncommitted paths under its workspace belong to other registered
/// languages: a shell script and a Go file are nothing ruff reads.
#[tokio::test]
async fn a_per_file_tool_runs_when_the_uncommitted_path_is_another_language() {
    let dir = tempfile::tempdir().expect("tempdir");
    let argv = ruff_fixture(dir.path());
    let a = dir.path().join("a.py");
    std::fs::write(&a, "a = 1\n").expect("a.py");

    let mut work = work_for(std::slice::from_ref(&a));
    work.uncommitted = [PathBuf::from("main.go"), PathBuf::from("setup.sh")]
        .into_iter()
        .collect();
    let (_findings, failures, _compiled) = deterministic::run(&work, dir.path()).await;

    assert!(
        failures.is_empty(),
        "a difference in another language must not refuse the tool, got {failures:?}"
    );
    assert_eq!(
        std::fs::read_to_string(&argv)
            .expect("recorded argv")
            .lines()
            .count(),
        1,
        "the tool must run exactly once"
    );
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

/// A fake `luacheck` that records each invocation's argv, a tool that reads
/// nothing but the files it is given (`reads_other_sources: false`), with a
/// `.luacheckrc` opting the root into it. Returns the argv record.
fn luacheck_fixture(root: &std::path::Path) -> PathBuf {
    let bin = root.join("lua_modules/bin/luacheck");
    std::fs::create_dir_all(bin.parent().unwrap()).expect("bin dir");
    let argv = root.join("argv.txt");
    write_executable(
        &bin,
        format!("#!/bin/sh\nprintf '%s\\n' \"$*\" >> {}\n", argv.display()),
    );
    std::fs::write(root.join(".luacheckrc"), "").expect("luacheckrc");
    argv
}

/// Runs luacheck over `file` with `uncommitted` differing and returns the
/// paths its refusal names, `None` when it ran.
async fn luacheck_refusal(
    root: &std::path::Path,
    file: &std::path::Path,
    uncommitted: &[&str],
) -> Option<Vec<PathBuf>> {
    let mut work = work_for(&[file.to_path_buf()]);
    work.uncommitted = uncommitted.iter().map(PathBuf::from).collect();
    let (_findings, failures, _compiled) = deterministic::run(&work, root).await;
    match failures.get(file) {
        Some(FailureReason::UncommittedChanges { tool, paths }) => {
            assert_eq!(tool, "luacheck");
            Some(paths.clone())
        }
        None => None,
        other => panic!("expected UncommittedChanges or nothing for {file:?}, got {other:?}"),
    }
}

/// Even a tool that reads only its files reads those files and its
/// configuration: an uncommitted one of either refuses it.
#[tokio::test]
async fn a_tool_reading_only_its_files_refuses_its_own_file_and_config() {
    let dir = tempfile::tempdir().expect("tempdir");
    let argv = luacheck_fixture(dir.path());
    let a = dir.path().join("a.lua");
    std::fs::write(&a, "local a = 1\n").expect("a.lua");

    assert_eq!(
        luacheck_refusal(dir.path(), &a, &["a.lua"]).await,
        Some(vec![PathBuf::from("a.lua")])
    );
    assert_eq!(
        luacheck_refusal(dir.path(), &a, &[".luacheckrc"]).await,
        Some(vec![PathBuf::from(".luacheckrc")])
    );
    assert!(!argv.exists(), "luacheck must not run on either");
}

/// A nearer config marker the working tree no longer holds, in any directory
/// between the file and the one that configured it on disk, is the commit's
/// configuration, so an
/// uncommitted one refuses the tool rather than letting it lint by the farther.
#[tokio::test]
async fn a_nearer_marker_missing_from_the_working_tree_refuses_the_tool() {
    let dir = tempfile::tempdir().expect("tempdir");
    let argv = luacheck_fixture(dir.path());
    std::fs::create_dir_all(dir.path().join("pkg/sub")).expect("pkg dirs");
    let a = dir.path().join("pkg/sub/a.lua");
    std::fs::write(&a, "local a = 1\n").expect("a.lua");

    assert_eq!(
        luacheck_refusal(dir.path(), &a, &["pkg/.luacheckrc"]).await,
        Some(vec![PathBuf::from("pkg/.luacheckrc")])
    );
    assert!(
        !argv.exists(),
        "luacheck must not run by the farther marker"
    );
}

/// A per-file tool that reads nothing but the files it is given
/// (`reads_other_sources: false`, as luacheck is) runs beside an uncommitted
/// file of its language: nothing it reads differs from the commit.
#[tokio::test]
async fn a_tool_reading_only_its_files_runs_beside_an_uncommitted_file_of_its_language() {
    let dir = tempfile::tempdir().expect("tempdir");
    let argv = luacheck_fixture(dir.path());
    let a = dir.path().join("a.lua");
    std::fs::write(&a, "local a = 1\n").expect("a.lua");

    let mut work = work_for(std::slice::from_ref(&a));
    work.uncommitted = [PathBuf::from("lib.lua")].into_iter().collect();
    let (_findings, failures, _compiled) = deterministic::run(&work, dir.path()).await;

    assert!(
        failures.is_empty(),
        "an uncommitted file luacheck never reads must not refuse it, got {failures:?}"
    );
    assert_eq!(
        std::fs::read_to_string(&argv)
            .expect("recorded argv")
            .lines()
            .count(),
        1,
        "the tool must run exactly once"
    );
}

/// A whole-project tool (`accepts_files: false`) is invoked bare from its
/// workspace and reads all of it, so any differing path under the workspace
/// refuses the run - and one of another language outside it does not.
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

    let mut work = work_for(std::slice::from_ref(&file));
    work.uncommitted = [PathBuf::from("other.py")].into_iter().collect();
    let (_findings, failures, _compiled) = deterministic::run(&work, root).await;
    assert!(
        failures.is_empty(),
        "a difference outside the workspace must not refuse the tool, got {failures:?}"
    );
    assert_eq!(
        std::fs::read_to_string(&counter)
            .expect("counter")
            .lines()
            .count(),
        1,
        "the tool must run once its workspace no longer differs"
    );
}

/// A whole-project tool reads beyond its workspace too, through a path
/// dependency or a relative import, so an uncommitted file of its language
/// outside the workspace refuses it.
#[tokio::test]
async fn a_whole_project_tool_refuses_its_language_outside_its_workspace() {
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
