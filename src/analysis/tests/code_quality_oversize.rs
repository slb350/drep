//! The payload size ceiling, enforced for every input mode.
//!
//! `PAYLOAD_MAX_BYTES` used to be `WHOLE_FILE_MAX_BYTES` in `cli::check` and
//! was consulted only during paths-mode input resolution. `--staged` and
//! `--diff` — the two modes a commit gate actually runs in — never saw it, so
//! a newly-added multi-megabyte file was rendered whole and sent to the model.
//! These tests reach the analyzer through hunks, which is the shape both diff
//! modes produce, so they fail against the old placement.

use std::path::{Path, PathBuf};

use wiremock::MockServer;

use crate::analysis::payload::{PAYLOAD_MAX_BYTES, render};
use crate::analysis::result::FailureReason;
use crate::diff::hunks::{Hunk, HunkLine};
use crate::languages;
use crate::test_support::request_count;

use super::support::analyzer_for;

/// One hunk of added lines whose rendered payload is at least `target` bytes.
///
/// Built from added lines rather than a single enormous line so it looks like
/// a real new file to every layer in between; each line costs its content plus
/// the gutter the payload renderer writes.
fn added_lines_hunk(file_path: &str, lines: usize) -> Vec<Hunk> {
    let body: Vec<HunkLine> = (0..lines)
        .map(|i| HunkLine::Added(format!("value_{i} = \"{}\"", "x".repeat(64))))
        .collect();
    vec![Hunk {
        file_path: PathBuf::from(file_path),
        old_start: 0,
        old_count: 0,
        new_start: 1,
        new_count: lines as u32,
        lines: body,
    }]
}

/// A diff-mode payload over the ceiling fails the file and never reaches the
/// endpoint.
///
/// The request count is the discriminating half: a version that renders the
/// payload, sends it, and only then complains would still report `TooLarge`,
/// while having already paid for and leaked the oversized request.
#[tokio::test]
async fn an_oversize_diff_payload_fails_the_file_without_calling_the_endpoint() {
    let server = MockServer::start().await;
    // No mock is mounted: any request would 404 and surface as a transport
    // failure, which is a different `FailureReason` than the one asserted.
    let (analyzer, _dir) = analyzer_for(&server);

    // ~80 bytes of content plus ~10 of gutter per line.
    let hunks = added_lines_hunk("src/huge.py", ((PAYLOAD_MAX_BYTES / 80) as usize) + 500);
    let result = analyzer.analyze_file(&hunks).await;

    assert!(
        result.findings.is_empty(),
        "an unanalyzed file produces no findings, got {:?}",
        result.findings
    );
    let reason = result
        .failed_files
        .get(&PathBuf::from("src/huge.py"))
        .expect("an oversize payload must fail the file, not skip it");
    match reason {
        FailureReason::PayloadTooLarge { bytes, limit } => {
            assert_eq!(*limit, PAYLOAD_MAX_BYTES);
            assert!(
                *bytes > PAYLOAD_MAX_BYTES,
                "reported size {bytes} must exceed the limit {limit}"
            );
        }
        other => panic!("expected PayloadTooLarge, got {other:?}"),
    }
    assert_eq!(
        request_count(&server).await,
        0,
        "an oversize payload must never be sent"
    );
}

/// Hunks whose rendered payload is **exactly** `target` bytes.
///
/// The comparison is `>`, so only a fixture sitting on the boundary can tell it
/// from `>=`. Built by rendering a one-line hunk to measure the fixed overhead
/// (header, scope sentence, gutter) and then padding that line by the
/// remainder — content is written into the payload verbatim, so one added byte
/// of content is one added byte of payload.
fn hunks_rendering_to_exactly(file_path: &str, target: u64) -> Vec<Hunk> {
    let build = |width: usize| {
        vec![Hunk {
            file_path: PathBuf::from(file_path),
            old_start: 0,
            old_count: 0,
            new_start: 1,
            new_count: 1,
            lines: vec![HunkLine::Added("b".repeat(width))],
        }]
    };
    let language = languages::detect(Path::new(file_path)).expect("a registered language");
    let overhead = render(language, &build(0))
        .expect("a non-empty hunk renders")
        .text
        .len() as u64;
    let padding = target
        .checked_sub(overhead)
        .expect("target must leave room for the payload's fixed overhead");
    let hunks = build(padding as usize);

    let actual = render(language, &hunks)
        .expect("a non-empty hunk renders")
        .text
        .len() as u64;
    assert_eq!(
        actual, target,
        "the fixture must sit exactly on the boundary, or it cannot \
         distinguish `>` from `>=`"
    );
    hunks
}

/// A payload of exactly `PAYLOAD_MAX_BYTES` is sent; one byte more is not.
///
/// Pins the comparison itself. Both halves in one test because either alone
/// admits a wrong operator: the accept half alone passes for a check that never
/// fires, and the reject half alone passes for `>=`.
#[tokio::test]
async fn a_payload_exactly_at_the_ceiling_is_sent_and_one_byte_over_is_not() {
    let server = MockServer::start().await;
    let (analyzer, _dir) = analyzer_for(&server);

    let exact = hunks_rendering_to_exactly("src/exact.py", PAYLOAD_MAX_BYTES);
    let result = analyzer.analyze_file(&exact).await;
    assert!(
        !matches!(
            result.failed_files.get(&PathBuf::from("src/exact.py")),
            Some(FailureReason::PayloadTooLarge { .. })
        ),
        "a payload of exactly the limit is within it"
    );
    assert_eq!(
        request_count(&server).await,
        1,
        "the boundary payload must reach the endpoint"
    );

    let over = hunks_rendering_to_exactly("src/over.py", PAYLOAD_MAX_BYTES + 1);
    let result = analyzer.analyze_file(&over).await;
    match result.failed_files.get(&PathBuf::from("src/over.py")) {
        Some(FailureReason::PayloadTooLarge { bytes, limit }) => {
            assert_eq!(*bytes, PAYLOAD_MAX_BYTES + 1);
            assert_eq!(*limit, PAYLOAD_MAX_BYTES);
        }
        other => panic!("one byte over the limit must fail; got {other:?}"),
    }
    assert_eq!(
        request_count(&server).await,
        1,
        "still one: the over-limit payload must not have been sent"
    );
}

/// A payload comfortably under the ceiling is still sent.
///
/// Without this the ceiling could be implemented as "always too large" and the
/// test above would pass.
#[tokio::test]
async fn a_payload_under_the_ceiling_is_still_sent() {
    let server = MockServer::start().await;
    let (analyzer, _dir) = analyzer_for(&server);

    let hunks = added_lines_hunk("src/small.py", 10);
    let result = analyzer.analyze_file(&hunks).await;

    assert!(
        !matches!(
            result.failed_files.get(&PathBuf::from("src/small.py")),
            Some(FailureReason::PayloadTooLarge { .. })
        ),
        "a small payload must not be rejected for size"
    );
    assert_eq!(
        request_count(&server).await,
        1,
        "a payload under the ceiling must reach the endpoint"
    );
}
