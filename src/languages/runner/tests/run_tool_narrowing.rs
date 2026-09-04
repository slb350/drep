//! Narrowing a whole-project tool's findings to the files drep was asked to check.

use super::support::*;
use crate::languages::runner::*;

/// A whole-project tool's findings are narrowed to the files being checked.
///
/// It reports on the entire crate, so without the filter a commit gate would
/// block on pre-existing issues in code the commit never touched - unfixable
/// by the author, and every commit would fail until the whole crate was clean.
/// The `./` prefix on one requested path is deliberate: the dash-guard adds it,
/// and the tool reports paths without it.
#[tokio::test]
async fn a_whole_project_tools_findings_are_narrowed_to_the_requested_files() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    // `lines` format: each line names a file the tool has an opinion about.
    install_stub(
        root,
        "wholeproject",
        "#!/bin/sh\nprintf 'wanted.rs\\nuntouched.rs\\nalso_wanted.rs\\n'\n",
    );
    let spec = whole_project_lines_spec();

    let outcome = run_tool(
        &spec,
        root,
        &["wanted.rs".to_owned(), "./also_wanted.rs".to_owned()],
    )
    .await;
    assert_eq!(outcome.status, ToolStatus::Ok, "detail: {}", outcome.detail);

    let mut reported: Vec<&str> = outcome
        .findings
        .iter()
        .map(|f| f.file_path.as_str())
        .collect();
    reported.sort_unstable();
    assert_eq!(
        reported,
        vec!["also_wanted.rs", "wanted.rs"],
        "untouched.rs was not asked about, so its finding must be dropped"
    );
}

/// A tool that *does* accept files keeps every finding it reports.
///
/// The other half of the filter: applying it unconditionally would silently
/// drop findings whenever a tool reported a path in a different but equivalent
/// form, so the narrowing must be scoped to the tools that need it.
#[tokio::test]
async fn a_file_taking_tools_findings_are_not_filtered() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    install_stub(
        root,
        "perfile",
        "#!/bin/sh\nprintf 'somewhere/else.rs\\n'\n",
    );
    let spec = per_file_lines_spec();

    let outcome = run_tool(&spec, root, &["asked.rs".to_owned()]).await;
    assert_eq!(outcome.status, ToolStatus::Ok, "detail: {}", outcome.detail);
    assert_eq!(
        outcome.findings.len(),
        1,
        "a per-file tool only reports on what it was given, so nothing is dropped"
    );
}

/// A whole-project tool that answers with absolute paths still has its
/// findings kept.
///
/// `plan_tasks` builds each argument by stripping `workspace_root`, so the
/// caller's list is workspace-relative, while `dotnet format` prints the
/// absolute path on every diagnostic line. Compared as strings those never
/// match, so the filter emptied the vector and every C# file came back clean -
/// a tool that ran, found real defects, and reported a pass.
#[tokio::test]
async fn a_whole_project_tools_absolute_paths_still_match_the_requested_files() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    // The stub prints the absolute path of the file it was never handed,
    // exactly as `dotnet format` does.
    let absolute = root.join("asked.rs");
    install_stub(
        root,
        "wholeproject",
        &format!(
            "#!/bin/sh\nprintf '%s\\n' '{}'\n",
            absolute.to_string_lossy()
        ),
    );
    let spec = whole_project_lines_spec();

    let outcome = run_tool(&spec, root, &["asked.rs".to_owned()]).await;
    assert_eq!(outcome.status, ToolStatus::Ok, "detail: {}", outcome.detail);
    assert_eq!(
        outcome.findings.len(),
        1,
        "an absolute path naming the requested file is the requested file"
    );
}

/// The narrowing still drops a whole-project tool's findings in files the
/// commit did not touch, absolute or not.
#[tokio::test]
async fn a_whole_project_tools_untouched_files_are_still_dropped() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let elsewhere = root.join("untouched.rs");
    install_stub(
        root,
        "wholeproject",
        &format!(
            "#!/bin/sh\nprintf '%s\\n' '{}'\n",
            elsewhere.to_string_lossy()
        ),
    );
    let spec = whole_project_lines_spec();

    let outcome = run_tool(&spec, root, &["asked.rs".to_owned()]).await;
    assert_eq!(outcome.status, ToolStatus::Ok, "detail: {}", outcome.detail);
    assert!(
        outcome.findings.is_empty(),
        "a commit gate must not block on a file the commit never touched"
    );
}

/// A whole-project tool that reports paths through a resolved symlink still
/// matches the requested files.
///
/// The runner spawns the tool with `current_dir(workspace_root)`, and a tool
/// that derives absolute paths from its own cwd spells them through resolved
/// symlinks: `/private/var/...` where drep's workspace says `/var/...` on
/// macOS, or any symlinked checkout. Byte-exact comparison misses those
/// spellings, and every finding is narrowed away - the filter reporting a
/// clean run over files the tool flagged. tflint `--recursive` emits
/// `../..`-carrying relative paths under the same resolution, which the
/// canonical form resolves the same way.
#[cfg(unix)]
#[tokio::test]
async fn findings_match_when_the_workspace_path_is_a_symlink() {
    let dir = tempfile::tempdir().expect("tempdir");
    let real = dir.path().join("real");
    std::fs::create_dir(&real).expect("real dir");
    let link = dir.path().join("link");
    std::os::unix::fs::symlink(&real, &link).expect("symlink");
    std::fs::write(real.join("asked.rs"), "fn main() {}\n").expect("source");

    // The stub answers with the canonical spelling, as a tool deriving the
    // path from its own cwd does.
    let canonical = real.join("asked.rs").canonicalize().expect("canonical");
    install_stub(
        &real,
        "wholeproject",
        &format!(
            "#!/bin/sh\nprintf '%s\\n' '{}'\n",
            canonical.to_string_lossy()
        ),
    );
    let spec = whole_project_lines_spec();

    let outcome = run_tool(&spec, &link, &["asked.rs".to_owned()]).await;
    assert_eq!(outcome.status, ToolStatus::Ok, "detail: {}", outcome.detail);
    assert_eq!(
        outcome.findings.len(),
        1,
        "a canonical spelling of a requested file is the requested file"
    );
}
