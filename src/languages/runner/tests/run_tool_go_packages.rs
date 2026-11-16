//! `go vet`'s package-shaped invocation: what it is handed, which requested files it covers, and which of its findings survive narrowing.

use super::support::logged_argv;
use crate::languages::runner::*;
use crate::test_support::fake_go::{self, Vet};

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

fn reported(outcome: &ToolOutcome) -> Vec<&str> {
    outcome
        .findings
        .iter()
        .map(|f| f.file_path.as_str())
        .collect()
}

/// Each requested file's package directory is handed over once, sorted, after the declared flags.
///
/// See [`ToolArguments::GoPackages`] for why `go vet` needs directories. A file at the workspace root sits in `.`, and a directory whose name begins with `-` is guarded by the same `./` every directory carries.
#[tokio::test]
async fn each_package_directory_is_handed_over_once() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let files = [
        "internal/b/y.go",
        "cmd/a/main.go",
        "internal/b/x.go",
        "root.go",
        "-dash/z.go",
    ];
    let log = fake_go::workspace(root, &files, Vet::default());

    let outcome = run_tool(&fake_go::spec(), root, &strings(&files)).await;
    assert_eq!(outcome.status, ToolStatus::Ok, "detail: {}", outcome.detail);
    assert_eq!(
        logged_argv(&log),
        vec!["vet", ".", "./-dash", "./cmd/a", "./internal/b"]
    );
    assert!(outcome.excluded.is_empty());
}

/// A diff spanning two packages is vetted, not failed: the push that 3.1.4 blocked.
///
/// 3.1.4 handed `go vet` the changed files themselves, and `go vet` refuses files from more than one directory with "named files must all be in one directory". That reached `drep check` as `Unavailable` for every file, so the gate exited 2. Directories are handed over instead, and a finding is attributed to the file it points at.
#[tokio::test]
async fn a_diff_spanning_two_packages_is_vetted_without_error() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let files = ["cmd/healthgate/main.go", "cmd/temporal-gateway/main.go"];
    fake_go::workspace(root, &files, Vet::default());

    let outcome = run_tool(&fake_go::spec(), root, &strings(&files)).await;
    assert_eq!(outcome.status, ToolStatus::Ok, "detail: {}", outcome.detail);
    assert!(outcome.findings.is_empty(), "{:?}", outcome.findings);
    assert!(outcome.compilation_succeeded);

    fake_go::workspace(
        root,
        &files,
        Vet {
            stderr: "cmd/temporal-gateway/main.go:7:2: unreachable code",
            exit: 1,
        },
    );
    let outcome = run_tool(&fake_go::spec(), root, &strings(&files)).await;
    assert_eq!(outcome.status, ToolStatus::Ok, "detail: {}", outcome.detail);
    assert_eq!(reported(&outcome), vec!["cmd/temporal-gateway/main.go"]);
}

/// A file build constraints exclude is not vetted and is reported as excluded, and its directory is left out of the vet call.
///
/// `go vet ./tools` on a directory whose only file is `//go:build tools` fails with "build constraints exclude all Go files", and that one failure aborts every other package in the same call. CI's `go vet ./...` never vets such a file either.
#[tokio::test]
async fn a_file_the_build_excludes_is_reported_as_excluded_and_not_vetted() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let log = fake_go::workspace(
        root,
        &["tools/tools_ignored.go", "pkg/a.go"],
        Vet::default(),
    );

    let outcome = run_tool(
        &fake_go::spec(),
        root,
        &strings(&["tools/tools_ignored.go", "pkg/a.go"]),
    )
    .await;
    assert_eq!(outcome.status, ToolStatus::Ok, "detail: {}", outcome.detail);
    assert_eq!(logged_argv(&log), vec!["vet", "./pkg"]);
    assert_eq!(outcome.excluded, vec!["tools/tools_ignored.go"]);
    assert!(outcome.compilation_succeeded);
}

/// When the build excludes every requested file, vet is not run and the outcome is a skip, never a pass that claims compilation.
#[tokio::test]
async fn when_the_build_excludes_every_file_vet_is_not_run() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let log = fake_go::workspace(root, &["tools/tools_ignored.go"], Vet::default());

    let outcome = run_tool(
        &fake_go::spec(),
        root,
        &strings(&["tools/tools_ignored.go"]),
    )
    .await;
    assert_eq!(
        outcome.status,
        ToolStatus::Skipped,
        "detail: {}",
        outcome.detail
    );
    assert!(logged_argv(&log).is_empty(), "vet must not run");
    assert_eq!(outcome.excluded, vec!["tools/tools_ignored.go"]);
    assert!(!outcome.compilation_succeeded);
}

/// A package that fails to load reports its load errors as findings wherever they point, and vet is not run.
///
/// An unresolvable import in one package aborts `go vet` for every package in the call, so running it would analyze nothing. The error usually sits in a file the commit did not touch, and narrowing it away reported a clean pass over code that was never vetted.
#[tokio::test]
async fn a_package_that_fails_to_load_reports_its_errors_and_vet_is_not_run() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let log = fake_go::workspace(
        root,
        &["broken/a.go", "broken/b.go", "pkg/a.go"],
        Vet::default(),
    );

    let outcome = run_tool(
        &fake_go::spec(),
        root,
        &strings(&["broken/a.go", "pkg/a.go"]),
    )
    .await;
    assert_eq!(outcome.status, ToolStatus::Ok, "detail: {}", outcome.detail);
    assert!(logged_argv(&log).is_empty(), "vet must not run");
    assert_eq!(reported(&outcome), vec!["broken/b.go"]);
    assert_eq!(outcome.findings[0].line, 3);
    assert!(outcome.findings[0].message.contains("ex/nope"));
    assert!(!outcome.compilation_succeeded);
}

/// A type-check failure survives narrowing wherever it points, while an analyzer finding in an untouched file is still dropped.
///
/// A commit that changes `Foo(x int)` in `a.go` breaks the call in `b.go`, and `go vet ./pkg` reports it only there. The package did not compile, so `a.go` was not vetted either.
#[tokio::test]
async fn a_type_check_failure_in_an_untouched_file_is_kept() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    fake_go::workspace(
        root,
        &["pkg/a.go", "pkg/b.go", "other/c.go"],
        Vet {
            stderr: "# ex/pkg\nvet: pkg/b.go:3:29: not enough arguments in call to Foo\nother/c.go:5:2: fmt.Printf format %d has arg \"s\" of wrong type string",
            exit: 1,
        },
    );

    let outcome = run_tool(&fake_go::spec(), root, &strings(&["pkg/a.go"])).await;
    assert_eq!(outcome.status, ToolStatus::Ok, "detail: {}", outcome.detail);
    assert_eq!(reported(&outcome), vec!["pkg/b.go"]);
    assert_eq!(outcome.findings[0].kind, TYPE_CHECK_KIND);
}

/// A file whose `//go:build` line does not parse is in the package as far as `go list` can tell, so it is vetted, not excluded: `go list` files it under `InvalidGoFiles` with a position-less package error.
///
/// Treating it as excluded would turn a file `go vet` rejects into a skip.
#[tokio::test]
async fn a_file_with_a_malformed_build_line_is_reported_not_excluded() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let log = fake_go::workspace(root, &["pkg/a.go", "pkg/bad_invalid.go"], Vet::default());

    let outcome = run_tool(&fake_go::spec(), root, &strings(&["pkg/bad_invalid.go"])).await;
    assert_eq!(outcome.status, ToolStatus::Ok, "detail: {}", outcome.detail);
    assert!(outcome.excluded.is_empty(), "{:?}", outcome.excluded);
    assert!(logged_argv(&log).is_empty(), "vet must not run");
    assert_eq!(reported(&outcome), vec!["pkg/bad_invalid.go"]);
    assert!(outcome.findings[0].message.contains("//go:build"));
}

/// A directory `go list` cannot place in the workspace, such as a module `go.work` does not use, leaves drep not knowing what vet would cover. That is `Unavailable` with go's own reason, not a parse error about a missing field.
#[tokio::test]
async fn a_directory_go_cannot_place_is_unavailable_with_its_reason() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    fake_go::workspace(root, &["outside/a.go", "pkg/a.go"], Vet::default());

    let outcome = run_tool(
        &fake_go::spec(),
        root,
        &strings(&["outside/a.go", "pkg/a.go"]),
    )
    .await;
    assert_eq!(outcome.status, ToolStatus::Unavailable);
    assert!(
        outcome.detail.contains("not one of the workspace modules"),
        "detail: {}",
        outcome.detail
    );
}

/// An analyzer finding in a sibling of the requested file is narrowed away: `go vet ./pkg` reports on every file in the package, test files included.
#[tokio::test]
async fn an_analyzer_finding_in_a_sibling_file_is_dropped() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    fake_go::workspace(
        root,
        &["pkg/asked.go", "pkg/sibling_test.go"],
        Vet {
            stderr: "pkg/asked.go:4:2: unreachable code\npkg/sibling_test.go:9:2: unreachable code",
            exit: 1,
        },
    );

    let outcome = run_tool(&fake_go::spec(), root, &strings(&["pkg/asked.go"])).await;
    assert_eq!(outcome.status, ToolStatus::Ok, "detail: {}", outcome.detail);
    assert_eq!(reported(&outcome), vec!["pkg/asked.go"]);
}

/// `go list` writes an error's position as `file:line:col`, sometimes `file:line`, and leaves it empty when the error has none. Split from the right, so a Windows drive letter stays in the file, and a position that is not numeric is all file.
#[test]
fn a_load_error_position_is_split_from_the_right() {
    use crate::languages::runner::go_packages::position;
    assert_eq!(
        position("broken/b.go:3:8"),
        Some(("broken/b.go", 3, Some(8)))
    );
    assert_eq!(position("broken/b.go:3"), Some(("broken/b.go", 3, None)));
    assert_eq!(
        position(r"C:\src\b.go:3:8"),
        Some((r"C:\src\b.go", 3, Some(8)))
    );
    assert_eq!(position("broken/b.go"), Some(("broken/b.go", 1, None)));
    assert_eq!(position(""), None);
}
