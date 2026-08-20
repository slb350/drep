//! Assembling a response that arrives in fragments.
//!
//! open-agent-sdk 0.10.0 delivers streamed text one `StreamEvent` per delta
//! while the stream is open; 0.9.x concatenated the whole response and emitted
//! a single `ContentBlock::Text` at the end. The types did not change, so the
//! compiler cannot tell the two apart, and `run_one_query` is the only place
//! the difference can be observed: it must `push_str` every text block it
//! receives, in order, with nothing between them.
//!
//! The first two tests deliver identical bytes as one delta and as several,
//! and compare the results, so what is pinned is invariance rather than a
//! hardcoded answer: reading only the first block, keeping only the last, and
//! joining the fragments with a separator each produce a different
//! `Extracted` from the one-delta delivery of the same bytes. The third pins
//! the terminating event, which fragmentation must not multiply.

use crate::llm::client::{Extracted, LlmError};
use crate::test_support::{
    cfg_for, fast_retry_client, request_count, server_finishing_with, server_returning,
};

/// One answer, split at positions a delta boundary would never respect.
///
/// The breaks fall mid-word in the prose, inside the opening fence, inside the
/// `findings` key, inside a string value and inside a number. Splitting inside
/// the key and the number is deliberate. A join that inserts *anything*
/// between fragments corrupts a key or a numeric literal, where between two
/// tokens it would only add whitespace the parser forgives - so the parse
/// fails rather than succeeding on text the model never sent. A newline is
/// the plausible separator: it is what the SDK's own wire builder applied
/// when it joined whole text blocks.
const FRAGMENTS: &[&str] = &[
    "Here is wh",
    "at I fou",
    "nd.\n\n``",
    "`json\n{\"find",
    "ings\": [{\"line\": 1",
    "2, \"severity\": \"warn",
    "ing\", \"message\": \"unwrap on a Res",
    "ult\"}]}\n``",
    "`\n",
];

/// The same object, cut off before it closes, and unfenced.
///
/// Unfenced because a cut response has no closing fence, so the fence
/// strategy does not match and the balancer is handed the whole body -
/// including any prose preamble, which does not balance. The cut falls after
/// a string literal ends, which is what the balancer can recover, and the
/// fragment boundaries inside it are the same awkward ones [`FRAGMENTS`]
/// uses.
const TRUNCATED: &[&str] = &[
    "{\"find",
    "ings\": [{\"line\": 1",
    "2, \"severity\": \"warn",
    "ing\", \"message\": \"unwrap on a Res",
    "ult\"",
];

/// The extracted JSON does not depend on where the deltas fall.
#[tokio::test]
async fn fragmented_text_extracts_what_one_delta_extracts() {
    let single_delta = FRAGMENTS.concat();
    let one = server_returning(&[single_delta.as_str()]).await;
    let many = server_returning(FRAGMENTS).await;

    let from_one = fast_retry_client(&cfg_for(&one, "m", 1))
        .complete_json("sys", "user")
        .await
        .expect("a fenced object parses");
    let from_many = fast_retry_client(&cfg_for(&many, "m", 1))
        .complete_json("sys", "user")
        .await
        .expect("the same bytes parse however they were streamed");

    assert!(
        matches!(from_one, Extracted::Complete(_)),
        "the fixture must be a complete object, got {from_one:?}"
    );
    assert_eq!(
        from_many, from_one,
        "a response split across deltas must assemble to the same JSON"
    );
    // A prefix of the answer carries no JSON, so a client that kept only the
    // first block would spend every no-JSON attempt before failing. One
    // request is what proves the first response was accepted.
    assert_eq!(request_count(&many).await, 1);
}

/// A truncated answer is still truncated, and still not retried.
///
/// The brace-balancing path reads the *assembled* text: a fragment boundary
/// inside the unterminated tail must not turn a recoverable truncation into
/// prose, nor prose into a recoverable truncation.
#[tokio::test]
async fn a_truncated_answer_survives_fragmentation() {
    let single_delta = TRUNCATED.concat();
    let one = server_returning(&[single_delta.as_str()]).await;
    let many = server_returning(TRUNCATED).await;

    let from_one = fast_retry_client(&cfg_for(&one, "m", 1))
        .complete_json("sys", "user")
        .await
        .expect("an unterminated object is recovered by brace balancing");
    let from_many = fast_retry_client(&cfg_for(&many, "m", 1))
        .complete_json("sys", "user")
        .await
        .expect("and is recovered identically when it arrives in pieces");

    assert!(
        matches!(from_one, Extracted::Truncated(_)),
        "the fixture must be the truncated case, got {from_one:?}"
    );
    assert_eq!(from_many, from_one);
    assert_eq!(request_count(&many).await, 1);
}

/// The finish reason still arrives once, after the last fragment.
///
/// `run_one_query` keeps one `finish` and the text separately; fragmentation
/// multiplies the text events without multiplying the `Finish` event, and a
/// capped response with no JSON in it must still end the attempt as
/// `ModelStopped` rather than being asked again.
#[tokio::test]
async fn a_capped_fragmented_response_is_not_retried() {
    let server = server_finishing_with(&["I ran ou", "t of room befo"], "length").await;

    let err = fast_retry_client(&cfg_for(&server, "m", 1))
        .complete_json("sys", "user")
        .await
        .expect_err("prose that hit the cap produced no JSON");

    assert!(
        matches!(&err, LlmError::ModelStopped { finish, .. } if finish == "length"),
        "got {err:?}"
    );
    assert_eq!(
        request_count(&server).await,
        1,
        "the same request hits the same cap, so it must not be asked again"
    );
}
