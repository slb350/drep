//! Failures: criteria 11, 12, 13, 14, 15, 19, 20.
//!
//! The five classes of "we do not understand the response" — out-of-range
//! lines, unknown severity, missing required field, transport failure, and
//! a missing `issues` field. Each test pins that the right axis flips:
//! `findings` is empty, `failed_files` does (or does not) contain the
//! file, and `dropped_out_of_range` is the counter that distinguishes
//! "dropped a model misreport" from "did not understand the response".

use std::path::PathBuf;

use tempfile::TempDir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::analysis::findings::Severity;
use crate::diff::hunks::Hunk;
use crate::llm::cache::Cache;

use super::support::analyzer_with_fast_retry;
use super::support::{analyzer_for, hunks_for_python_at, hunks_for_python_at_two_lines};
use crate::test_support::{cfg_for, mount_sse, request_count, server_returning, sse};

/// Criterion 11: a response with two issues yields two findings with the
/// right `kind`/`line`/`message`/`suggestion`.
#[tokio::test]
async fn two_issues_yield_two_findings_with_full_fields() {
    let server = MockServer::start().await;
    let body = "{\"issues\": [\
        {\"line\": 100, \"severity\": \"high\", \"category\": \"bug\", \
         \"message\": \"first bug\", \"suggestion\": \"fix it\"}, \
        {\"line\": 101, \"severity\": \"medium\", \"category\": \"performance\", \
         \"message\": \"second issue\", \"suggestion\": \"optimize\"}\
    ], \"summary\": \"two findings\"}";
    mount_sse(
        &server,
        ResponseTemplate::new(200).set_body_raw(sse(&[body]), "text/event-stream"),
    )
    .await;

    let (analyzer, _dir) = analyzer_for(&server);
    let result = analyzer
        .analyze_file(&hunks_for_python_at_two_lines())
        .await;

    assert_eq!(
        result.findings.len(),
        2,
        "two issues must yield two findings"
    );
    let first = &result.findings[0];
    assert_eq!(first.kind, "bug");
    assert_eq!(first.line, 100);
    assert_eq!(first.message, "first bug");
    assert_eq!(first.suggestion.as_deref(), Some("fix it"));
    let second = &result.findings[1];
    assert_eq!(second.kind, "performance");
    assert_eq!(second.line, 101);
    assert_eq!(second.message, "second issue");
    assert_eq!(second.suggestion.as_deref(), Some("optimize"));
}

/// Criterion 12: severity mapping. One issue of each severity, in order
/// `critical|high|medium|low|info`, must map to
/// `[Error, Error, Warning, Info, Info]`. Asserting all five in one test
/// rules out a hardcoded default.
#[tokio::test]
async fn severity_string_maps_to_severity_enum() {
    let server = MockServer::start().await;
    let body = "{\"issues\": [\
        {\"line\": 100, \"severity\": \"critical\", \"category\": \"c\", \"message\": \"m\"}, \
        {\"line\": 100, \"severity\": \"high\", \"category\": \"c\", \"message\": \"m\"}, \
        {\"line\": 100, \"severity\": \"medium\", \"category\": \"c\", \"message\": \"m\"}, \
        {\"line\": 100, \"severity\": \"low\", \"category\": \"c\", \"message\": \"m\"}, \
        {\"line\": 100, \"severity\": \"info\", \"category\": \"c\", \"message\": \"m\"}\
    ], \"summary\": \"\"}";
    mount_sse(
        &server,
        ResponseTemplate::new(200).set_body_raw(sse(&[body]), "text/event-stream"),
    )
    .await;

    let (analyzer, _dir) = analyzer_for(&server);
    let result = analyzer.analyze_file(&hunks_for_python_at(100)).await;

    let got: Vec<Severity> = result.findings.iter().map(|f| f.severity).collect();
    assert_eq!(
        got,
        vec![
            Severity::Error,
            Severity::Error,
            Severity::Warning,
            Severity::Info,
            Severity::Info,
        ],
        "critical/high → Error, medium → Warning, low/info → Info, in that order"
    );
}

/// Criterion 13: an out-of-range line is dropped, not clamped. The payload
/// used here has `valid_lines = {100..=110}`; the response reports `line: 5`.
/// The finding must be dropped, the file must NOT be marked failed, and the
/// dropped count must increment. Critically, no finding may appear with line
/// 100 — that is what clamping would have produced.
#[tokio::test]
async fn out_of_range_line_is_dropped_not_clamped() {
    use crate::diff::hunks::HunkLine;

    let server = MockServer::start().await;
    let body = "{\"issues\": [\
        {\"line\": 5, \"severity\": \"critical\", \"category\": \"bug\", \"message\": \"oops\"}\
    ], \"summary\": \"\"}";
    mount_sse(
        &server,
        ResponseTemplate::new(200).set_body_raw(sse(&[body]), "text/event-stream"),
    )
    .await;

    let (analyzer, _dir) = analyzer_for(&server);
    let hunks = vec![Hunk {
        file_path: PathBuf::from("src/lib.py"),
        old_start: 99,
        old_count: 0,
        new_start: 100,
        new_count: 11,
        // 11 numbered lines: 100..=110. Line 5 is out of range.
        lines: (100..=110u32)
            .map(|n| HunkLine::Added(format!("line {n}")))
            .collect(),
    }];
    let result = analyzer.analyze_file(&hunks).await;

    assert!(
        result.findings.is_empty(),
        "no finding may be emitted on an out-of-range line, got {:?}",
        result.findings
    );
    assert!(
        result.failed_files.is_empty(),
        "an out-of-range line is a model misreport, not a failure, got {:?}",
        result.failed_files
    );
    assert_eq!(
        result.dropped_out_of_range, 1,
        "the drop must be counted, not silent"
    );
    // Crucially: no finding with line 100. A clamping implementation
    // would have produced one.
    assert!(
        !result.findings.iter().any(|f| f.line == 100),
        "no clamping: an out-of-range line must never appear on a valid line"
    );
}

/// Criterion 14: an unknown severity makes the file unanalyzed. The record
/// is skipped, the file is marked failed, and `dropped_out_of_range` stays
/// zero — this is a different failure class from the out-of-range case.
#[tokio::test]
async fn unknown_severity_marks_the_file_unanalyzed() {
    let server = MockServer::start().await;
    let body = "{\"issues\": [\
        {\"line\": 100, \"severity\": \"blocker\", \"category\": \"bug\", \"message\": \"bad\"}\
    ], \"summary\": \"\"}";
    mount_sse(
        &server,
        ResponseTemplate::new(200).set_body_raw(sse(&[body]), "text/event-stream"),
    )
    .await;

    let (analyzer, _dir) = analyzer_for(&server);
    let result = analyzer.analyze_file(&hunks_for_python_at(100)).await;

    assert!(
        result.findings.is_empty(),
        "the malformed record must be skipped, got {:?}",
        result.findings
    );
    assert!(
        result
            .failed_files
            .contains_key(&PathBuf::from("src/lib.py")),
        "unknown severity must mark the file unanalyzed, got {:?}",
        result.failed_files
    );
    assert_eq!(
        result.dropped_out_of_range, 0,
        "unknown severity is not an out-of-range drop, got {}",
        result.dropped_out_of_range
    );
}

/// Criterion 15: a missing required field (`line`) behaves as criterion 14.
#[tokio::test]
async fn missing_required_field_marks_the_file_unanalyzed() {
    let server = MockServer::start().await;
    // No `line` field at all.
    let body = "{\"issues\": [\
        {\"severity\": \"high\", \"category\": \"bug\", \"message\": \"x\"}\
    ], \"summary\": \"\"}";
    mount_sse(
        &server,
        ResponseTemplate::new(200).set_body_raw(sse(&[body]), "text/event-stream"),
    )
    .await;

    let (analyzer, _dir) = analyzer_for(&server);
    let result = analyzer.analyze_file(&hunks_for_python_at(100)).await;

    assert!(result.findings.is_empty());
    assert!(
        result
            .failed_files
            .contains_key(&PathBuf::from("src/lib.py")),
        "missing field must mark the file unanalyzed, got {:?}",
        result.failed_files
    );
    assert_eq!(
        result.dropped_out_of_range, 0,
        "a missing field is a malformed record, not an out-of-range drop - \
         without this the two failure classes are indistinguishable here, got {}",
        result.dropped_out_of_range
    );
}

/// Criterion 19: a transport failure (mock returns 500 on every attempt)
/// yields no findings and the file in `failed_files`.
#[tokio::test]
async fn transport_failure_marks_the_file_failed() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(500).set_body_string("boom"))
        .mount(&server)
        .await;

    let dir = TempDir::new().expect("temp dir");
    let cache = Cache::new(dir.path().to_path_buf(), 30, 1024 * 1024);
    let mut cfg = cfg_for(&server, "m", 1);
    cfg.timeout_secs = 30;
    // Build the analyzer with a fast-retry client so the test does not
    // sleep through the default 1s backoff.
    let analyzer = analyzer_with_fast_retry(&cfg, cache);

    let result = analyzer.analyze_file(&hunks_for_python_at(100)).await;

    assert!(
        result.findings.is_empty(),
        "transport failure yields no findings, got {:?}",
        result.findings
    );
    assert!(
        result
            .failed_files
            .contains_key(&PathBuf::from("src/lib.py")),
        "transport failure must mark the file failed, got {:?}",
        result.failed_files
    );
}

/// Criterion 20: `issues` absent from the response object → no findings,
/// file in `failed_files`.
#[tokio::test]
async fn missing_issues_field_marks_the_file_failed() {
    let server = MockServer::start().await;
    let body = "{\"summary\": \"no issues here\"}";
    mount_sse(
        &server,
        ResponseTemplate::new(200).set_body_raw(sse(&[body]), "text/event-stream"),
    )
    .await;

    let (analyzer, _dir) = analyzer_for(&server);
    let result = analyzer.analyze_file(&hunks_for_python_at(100)).await;

    assert!(
        result.findings.is_empty(),
        "missing `issues` must yield no findings, got {:?}",
        result.findings
    );
    assert!(
        result
            .failed_files
            .contains_key(&PathBuf::from("src/lib.py")),
        "missing `issues` must mark the file failed, got {:?}",
        result.failed_files
    );
}

/// A line number too large for `u32` is malformed, not a silent clamp.
///
/// `u32::try_from(line).unwrap_or(u32::MAX)` used to land such a record in
/// `Dropped` *by accident*: `u32::MAX` is never in `valid_lines`, so it fell
/// out of the range check. That reports the file as analyzed. A line of
/// 2^32 is the same class of model artifact as a line of zero, which is
/// already `Malformed`, so it must be treated the same way.
#[tokio::test]
async fn line_beyond_u32_marks_the_file_unanalyzed() {
    let server = MockServer::start().await;
    let body = "{\"issues\": [\
        {\"line\": 4294967296, \"severity\": \"high\", \"category\": \"bug\", \"message\": \"m\"}\
    ], \"summary\": \"\"}";
    mount_sse(
        &server,
        ResponseTemplate::new(200).set_body_raw(sse(&[body]), "text/event-stream"),
    )
    .await;

    let (analyzer, _dir) = analyzer_for(&server);
    let result = analyzer.analyze_file(&hunks_for_python_at(100)).await;

    assert!(
        result.findings.is_empty(),
        "the record must be skipped, got {:?}",
        result.findings
    );
    assert!(
        result
            .failed_files
            .contains_key(&PathBuf::from("src/lib.py")),
        "a line beyond u32 must mark the file unanalyzed, got {:?}",
        result.failed_files
    );
    assert_eq!(
        result.dropped_out_of_range, 0,
        "this is a malformed record, not an out-of-range drop, got {}",
        result.dropped_out_of_range
    );
}

/// A schema-invalid response is not cached.
///
/// It is valid JSON, so it arrives as `Extracted::Complete` and the cache
/// would happily store it - and then serve the same file-level failure for the
/// whole TTL, with no request made to notice the endpoint had recovered. It
/// gets the same treatment as a truncated response for the same reason.
#[tokio::test]
async fn a_schema_invalid_response_is_not_cached() {
    // Valid JSON, no `issues` array: parses, then fails the schema.
    let server = server_returning(&["{\"summary\": \"nothing useful\"}"]).await;

    let (analyzer, _dir) = analyzer_for(&server);
    let first = analyzer.analyze_file(&hunks_for_python_at(100)).await;
    let second = analyzer.analyze_file(&hunks_for_python_at(100)).await;

    assert!(
        !first.failed_files.is_empty(),
        "first call must fail the file"
    );
    assert!(!second.failed_files.is_empty(), "second call must fail too");
    assert_eq!(
        request_count(&server).await,
        2,
        "a schema-invalid response must not be cached: the second call has to \
         ask again rather than replay the stored failure"
    );
}

/// A record that is BOTH out of range and malformed fails the file.
///
/// This is the case neither criterion 13 nor 14 constrains, and it is decided
/// by the order of the checks in `parse_issue`: shape before membership. An
/// unknown severity is evidence the response's vocabulary is wrong, which
/// contaminates the records we did accept - so it must fail the file even
/// though the record also cites a line we never sent. Checking membership
/// first would report a demonstrably schema-violating response as fully
/// understood.
#[tokio::test]
async fn an_out_of_range_record_with_a_bad_severity_still_fails_the_file() {
    let server = MockServer::start().await;
    // Line 5 is not in the payload (valid_lines is 100), and `blocker` is not
    // a severity we asked for.
    let body = "{\"issues\": [\
        {\"line\": 5, \"severity\": \"blocker\", \"category\": \"bug\", \"message\": \"m\"}\
    ], \"summary\": \"\"}";
    mount_sse(
        &server,
        ResponseTemplate::new(200).set_body_raw(sse(&[body]), "text/event-stream"),
    )
    .await;

    let (analyzer, _dir) = analyzer_for(&server);
    let result = analyzer.analyze_file(&hunks_for_python_at(100)).await;

    assert!(result.findings.is_empty());
    assert!(
        result
            .failed_files
            .contains_key(&PathBuf::from("src/lib.py")),
        "shape is checked before membership, so the bad severity wins, got {:?}",
        result.failed_files
    );
    assert_eq!(
        result.dropped_out_of_range, 0,
        "it is malformed, not a drop - counting it as a drop would report the \
         file as analyzed, got {}",
        result.dropped_out_of_range
    );
}
