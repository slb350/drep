//! `go vet`'s package-shaped outcome as the orchestrator merges it: files the build excludes leave the batch, and a type-check failure in an untouched file is named where the reader can find it.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use crate::analysis::findings::Finding;
use crate::analysis::result::FailureReason;
use crate::cli::check::deterministic::{PlannedFile, PlannedTask, merge_outcome, run_one};
use crate::languages::runner::ToolOutcome;
use crate::languages::spec::ToolSpec;
use crate::test_support::fake_go::{self, Vet};

static FAKE_GO_VET: LazyLock<ToolSpec> = LazyLock::new(fake_go::spec);

fn task(workspace: &Path, files: &[&str]) -> PlannedTask {
    PlannedTask {
        spec: &FAKE_GO_VET,
        workspace_root: workspace.to_path_buf(),
        files: files
            .iter()
            .map(|file| PlannedFile {
                original: workspace.join(file),
                absolute: workspace.join(file),
                argument: (*file).to_owned(),
            })
            .collect(),
    }
}

/// A file the build excludes is neither a failure nor compiled, while its vetted neighbour compiles.
///
/// Counting it compiled would let it suppress a model's "does not compile" claim about code no compiler saw, and failing it would exit 2 on every commit touching a `tools.go`.
#[tokio::test]
async fn a_file_the_build_excludes_is_neither_failed_nor_compiled() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    fake_go::workspace(
        root,
        &["pkg/a.go", "tools/tools_ignored.go"],
        Vet::default(),
    );

    let (outcome, files) = run_one(task(root, &["pkg/a.go", "tools/tools_ignored.go"]), root).await;
    let (failures, _findings, compiled) = merged(outcome, files);

    assert!(failures.is_empty(), "{failures:?}");
    assert_eq!(compiled, BTreeSet::from([root.join("pkg/a.go")]));
}

/// When `go vet` cannot run, only the files it was asked to cover fail; an excluded file was never going to be checked.
#[tokio::test]
async fn when_vet_is_unavailable_an_excluded_file_does_not_fail() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    fake_go::workspace(
        root,
        &["pkg/a.go", "tools/tools_ignored.go"],
        Vet {
            stderr: "go: something broke",
            exit: 1,
        },
    );

    let (outcome, files) = run_one(task(root, &["pkg/a.go", "tools/tools_ignored.go"]), root).await;
    let (failures, _findings, _compiled) = merged(outcome, files);

    let failed: Vec<&PathBuf> = failures.keys().collect();
    assert_eq!(failed, vec![&root.join("pkg/a.go")]);
}

/// A type-check failure in a file the commit did not touch is named relative to the repository, not to the Go module it was reported from.
///
/// `go vet` runs from the module root, so in a nested module it says `pkg/b.go` where the repository holds `sub/pkg/b.go`. Every other finding is renamed through the file the caller asked about; this one names a file nobody asked about.
#[tokio::test]
async fn a_type_check_failure_in_an_untouched_file_is_named_from_the_repository_root() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let module = root.join("sub");
    fake_go::workspace(
        &module,
        &["pkg/a.go", "pkg/b.go"],
        Vet {
            stderr: "# ex/pkg\nvet: pkg/b.go:3:29: not enough arguments in call to Foo",
            exit: 1,
        },
    );

    // The CLI passes the repository root as `.`, while the workspace it finds is absolute, so the root is handed over relative here too.
    let (outcome, _files) = run_one(task(&module, &["pkg/a.go"]), &relative_to_cwd(root)).await;

    let named: Vec<&str> = outcome
        .findings
        .iter()
        .map(|f| f.file_path.as_str())
        .collect();
    assert_eq!(named, vec!["sub/pkg/b.go"]);
}

/// `path` spelled relative to the current directory, climbing to the filesystem root with `..` first.
fn relative_to_cwd(path: &Path) -> PathBuf {
    let cwd = std::env::current_dir().expect("cwd");
    let mut relative: PathBuf = cwd.components().skip(1).map(|_| "..").collect();
    relative.push(path.strip_prefix("/").expect("absolute temp dir"));
    relative
}

/// What the orchestrator records for one outcome over `files`: failures, findings and compiled files.
fn merged(
    outcome: ToolOutcome,
    files: Vec<PathBuf>,
) -> (
    BTreeMap<PathBuf, FailureReason>,
    Vec<Finding>,
    BTreeSet<PathBuf>,
) {
    let (mut failures, mut findings, mut compiled) = (BTreeMap::new(), Vec::new(), BTreeSet::new());
    merge_outcome(outcome, files, &mut failures, &mut findings, &mut compiled);
    (failures, findings, compiled)
}
