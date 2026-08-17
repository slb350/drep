//! `CodeQualityAnalyzer` — criteria 8-22.
//!
//! Tests are split by topic across sibling files in this directory
//! (`code_quality_failures`, `code_quality_truncation`,
//! `code_quality_multi`) so the file stays under the 600 LOC limit. Each
//! test owns a `MockServer`, constructs an analyzer, and asserts both the
//! returned `AnalysisResult` and the mock's request count where the
//! criterion depends on the call actually happening (or not).

use std::collections::BTreeSet;
use std::path::PathBuf;

use wiremock::MockServer;

use crate::analysis::result::AnalysisResult;

use crate::diff::hunks::{Hunk, HunkLine};

use super::support::{analyzer_for, hunks_for_python_at};
use crate::test_support::{request_count, server_returning};

/// Criterion 8: a file whose extension no language claims returns an empty
/// result and makes no HTTP request.
#[tokio::test]
async fn unknown_extension_returns_empty_and_makes_no_request() {
    // Deliberately no mock mounted. If the analyzer wrongly issued a request
    // it would 404, which lands the file in `failed_files` - so the assertions
    // below catch the bug twice over. Mounting a clean response here would let
    // a wrongly-issued call succeed, leaving only `request_count` to notice.
    let server = MockServer::start().await;

    let (analyzer, _dir) = analyzer_for(&server);
    let hunks = vec![Hunk {
        file_path: PathBuf::from("notes.xyz"),
        old_start: 1,
        old_count: 0,
        new_start: 1,
        new_count: 1,
        lines: vec![HunkLine::Added("n/a".to_owned())],
    }];

    let result = analyzer.analyze_file(&hunks).await;

    assert!(
        result.findings.is_empty(),
        "no language means no analysis, got {:?}",
        result.findings
    );
    assert!(
        result.failed_files.is_empty(),
        "no language means no failure, got {:?}",
        result.failed_files
    );
    assert_eq!(
        request_count(&server).await,
        0,
        "an unknown extension must not produce an HTTP request"
    );
}

/// Criterion 9: an empty hunk slice returns an empty result and makes no
/// request.
#[tokio::test]
async fn empty_hunks_return_empty_and_make_no_request() {
    // Deliberately no mock mounted. If the analyzer wrongly issued a request
    // it would 404, which lands the file in `failed_files` - so the assertions
    // below catch the bug twice over. Mounting a clean response here would let
    // a wrongly-issued call succeed, leaving only `request_count` to notice.
    let server = MockServer::start().await;

    let (analyzer, _dir) = analyzer_for(&server);
    let result = analyzer.analyze_file(&[]).await;

    assert!(result.findings.is_empty());
    assert!(result.failed_files.is_empty());
    assert_eq!(
        request_count(&server).await,
        0,
        "empty hunks must not produce an HTTP request"
    );
}

/// Criterion 10: a clean response yields no findings and no failures.
/// "Clean" is a legitimate outcome, not a failure.
#[tokio::test]
async fn clean_response_yields_no_findings_and_no_failures() {
    let server = server_returning(&["{\"issues\": [], \"summary\": \"ok\"}"]).await;

    let (analyzer, _dir) = analyzer_for(&server);
    let result = analyzer.analyze_file(&hunks_for_python_at(100)).await;

    assert!(
        result.findings.is_empty(),
        "clean response is empty findings, got {:?}",
        result.findings
    );
    assert!(
        result.failed_files.is_empty(),
        "clean response is not a failure, got {:?}",
        result.failed_files
    );
    assert_eq!(
        result.dropped_out_of_range, 0,
        "clean response has no out-of-range drops"
    );
}

/// The `failed_files` field is a `BTreeSet` so the merge union is
/// correct. Pinning the type here rules out a refactor that swaps it
/// for a `Vec` and silently breaks the union semantics.
#[test]
fn failed_files_is_a_btreeset_in_the_returned_result() {
    let result = AnalysisResult::default();
    let _: BTreeSet<PathBuf> = result.failed_files;
}
