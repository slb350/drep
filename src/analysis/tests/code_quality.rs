//! `CodeQualityAnalyzer` — criteria 8-22.
//!
//! Tests are split by topic across sibling files in this directory
//! (`code_quality_failures`, `code_quality_truncation`,
//! `code_quality_multi`) so the file stays under the 600 LOC limit. Each
//! test owns a `MockServer`, constructs an analyzer, and asserts both the
//! returned `AnalysisResult` and the mock's request count where the
//! criterion depends on the call actually happening (or not).

use std::path::PathBuf;

use wiremock::MockServer;

use crate::analysis::result::FailureReason;

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

/// A public caller that supplies more than one file is partitioned safely.
#[tokio::test]
async fn mixed_file_hunks_are_analyzed_and_attributed_separately() {
    let finding = r#"{"issues":[{"line":1,"severity":"high","message":"found"}]}"#;
    let server = server_returning(&[finding]).await;
    let (analyzer, _dir) = analyzer_for(&server);
    let mut hunks = hunks_for_python_at(1);
    hunks.push(Hunk {
        file_path: PathBuf::from("other.py"),
        old_start: 1,
        old_count: 0,
        new_start: 1,
        new_count: 1,
        lines: vec![HunkLine::Added("different = True".to_owned())],
    });

    let result = analyzer.analyze_file(&hunks).await;

    assert_eq!(request_count(&server).await, 2);
    assert_eq!(result.findings.len(), 2, "findings: {:?}", result.findings);
    let paths: std::collections::BTreeSet<_> = result
        .findings
        .iter()
        .map(|finding| finding.file_path.as_str())
        .collect();
    assert_eq!(paths, ["other.py", "src/lib.py"].into_iter().collect());
}

/// Criterion 10: a clean response yields no findings and no failures.
/// "Clean" is a legitimate outcome, not a failure.
#[tokio::test]
async fn clean_response_yields_no_findings_and_no_failures() {
    let server = server_returning(&["{\"issues\": [], \"summary\": \"ok\"}"]).await;

    let (analyzer, _dir) = analyzer_for(&server);
    let result = analyzer.analyze_file(&hunks_for_python_at(100)).await;

    // Without this, an analyzer that never issued the request at all would
    // also produce empty findings and no failures, and this test would call
    // that a clean run.
    assert_eq!(
        request_count(&server).await,
        1,
        "a clean result must come from a response, not from never asking"
    );
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

/// An empty `suggestion` is `None`, not `Some("")`.
///
/// An empty suggestion is not a suggestion: rendered, `Some("")` prints a bare
/// `suggestion:` line under the finding with nothing after it. The optional
/// fields are the ones where "absent", "empty" and "wrong type" all have to
/// stay distinct, so each is pinned rather than assumed.
#[tokio::test]
async fn an_empty_suggestion_is_none_rather_than_an_empty_string() {
    let server = server_returning(&[
        r#"{"issues": [{"line": 100, "severity": "medium", "category": "style",
            "message": "m", "suggestion": ""}], "summary": "s"}"#,
    ])
    .await;
    let (analyzer, _dir) = analyzer_for(&server);

    let result = analyzer.analyze_file(&hunks_for_python_at(100)).await;
    assert!(
        result.failed_files.is_empty(),
        "an empty suggestion is legal"
    );
    assert_eq!(result.findings.len(), 1);
    assert_eq!(
        result.findings[0].suggestion, None,
        "an empty suggestion must not survive as Some(\"\")"
    );
}

/// A present-but-non-string `suggestion` makes the file unanalyzed.
///
/// The discriminating counterpart: absent and empty both mean "no suggestion",
/// but a `suggestion` of `7` is a response we did not understand. Treating it
/// as absent recorded a schema-violating response as fully understood — and
/// then cached it for the whole TTL.
#[tokio::test]
async fn a_non_string_suggestion_makes_the_file_unanalyzed() {
    let server = server_returning(&[
        r#"{"issues": [{"line": 100, "severity": "medium", "category": "style",
            "message": "m", "suggestion": 7}], "summary": "s"}"#,
    ])
    .await;
    let (analyzer, _dir) = analyzer_for(&server);

    let result = analyzer.analyze_file(&hunks_for_python_at(100)).await;
    let reason = result
        .failed_files
        .values()
        .next()
        .expect("a non-string suggestion is malformed");
    assert!(
        matches!(reason, FailureReason::MalformedFinding(detail) if detail.contains("suggestion")),
        "got {reason:?}"
    );
}

/// A present-but-non-string `category` makes the file unanalyzed.
///
/// Absent means "unknown"; a `category` of `7` does not. Defaulting both to
/// `"unknown"` reported a response whose schema we demonstrably did not
/// understand as a clean finding.
#[tokio::test]
async fn a_non_string_category_makes_the_file_unanalyzed() {
    let server = server_returning(&[
        r#"{"issues": [{"line": 100, "severity": "medium", "category": 7,
            "message": "m"}], "summary": "s"}"#,
    ])
    .await;
    let (analyzer, _dir) = analyzer_for(&server);

    let result = analyzer.analyze_file(&hunks_for_python_at(100)).await;
    let reason = result
        .failed_files
        .values()
        .next()
        .expect("a non-string category is malformed");
    assert!(
        matches!(reason, FailureReason::MalformedFinding(detail) if detail.contains("category")),
        "got {reason:?}"
    );
}

/// An absent `category` defaults to `"unknown"` and the file stays analyzed.
///
/// The third leg: without it, a rule that rejected every category would pass
/// both tests above.
#[tokio::test]
async fn an_absent_category_defaults_to_unknown() {
    let server = server_returning(&[
        r#"{"issues": [{"line": 100, "severity": "medium", "message": "m"}], "summary": "s"}"#,
    ])
    .await;
    let (analyzer, _dir) = analyzer_for(&server);

    let result = analyzer.analyze_file(&hunks_for_python_at(100)).await;
    assert!(
        result.failed_files.is_empty(),
        "an absent category is legal"
    );
    assert_eq!(result.findings[0].kind, "unknown");
}
