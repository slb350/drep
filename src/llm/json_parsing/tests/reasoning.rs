//! Tests for the leading-reasoning-block strip.
//!
//! The fixture in [`minimax_body`] is the shape a MiniMax M-series model
//! actually returns over the OpenAI-compatible endpoint: the whole trace inline
//! at the head of `message.content`, and a fenced code sample *inside* that
//! trace. A hand-written sample without the inner fence does not reproduce the
//! defect, because the defect is a fence-ordering interaction.

use super::super::*;
use serde_json::json;

/// A response whose reasoning precedes the answer and contains its own fence.
fn minimax_body() -> String {
    [
        "<think>",
        "The user wants a review. Looking at the code:",
        "```python",
        "def f(x):",
        "    return x / 0",
        "```",
        "That divides by zero, so I should report it.",
        "</think>",
        "```json",
        r#"{"issues": [{"line": 2, "message": "division by zero"}], "summary": "one issue"}"#,
        "```",
    ]
    .join("\n")
}

#[test]
fn a_leading_think_block_is_stripped_before_the_fence_ladder() {
    // Without the strip, FENCE_RE takes the ```python fence out of the
    // reasoning, every later strategy fails on Python source, and the whole
    // file comes back Unparseable.
    let extracted = extract_json(&minimax_body()).expect("the answer parses");

    let value = match extracted {
        Extracted::Complete(value) => value,
        Extracted::Truncated(value) => panic!("expected Complete, got Truncated({value})"),
    };
    assert_eq!(value["issues"].as_array().expect("issues").len(), 1);
    assert_eq!(value["issues"][0]["message"], json!("division by zero"));
}

#[test]
fn the_reasoning_text_never_reaches_the_parsed_value() {
    // Kills a strip that takes the wrong side of the tag pair: keeping the
    // reasoning and discarding the answer would still parse for some inputs.
    let value = match extract_json(&minimax_body()).expect("parses") {
        Extracted::Complete(value) => value,
        other => panic!("expected Complete, got {other:?}"),
    };

    assert_eq!(value["summary"], json!("one issue"));
    assert!(
        !value.to_string().contains("The user wants a review"),
        "reasoning leaked into the value: {value}"
    );
}

#[test]
fn a_think_tag_that_is_not_at_the_start_is_left_alone() {
    // A finding *about* a file containing the tag. Stripping here would rewrite
    // the model's answer, and an unanchored strip cannot tell the two apart.
    let input = r#"{"issues": [{"line": 1, "message": "remove the <think>debug</think> marker"}]}"#;

    let value = match extract_json(input).expect("parses") {
        Extracted::Complete(value) => value,
        other => panic!("expected Complete, got {other:?}"),
    };

    assert_eq!(
        value["issues"][0]["message"],
        json!("remove the <think>debug</think> marker")
    );
}

#[test]
fn an_unterminated_think_block_yields_no_json() {
    // The block never closed, so the answer never arrived. Stripping to
    // end-of-string would swallow the rest of the response and report a clean
    // parse of whatever happened to follow.
    let input = "<think>reasoning that never ends\n{\"issues\": []}";

    assert_eq!(
        extract_json(input),
        None,
        "an unterminated block is unparseable, not silently recovered"
    );
}

#[test]
fn a_response_with_no_think_block_passes_through_byte_for_byte() {
    // Pins that the OpenAI-compatible path is unchanged by the new code.
    let clean = "```json\n{\"issues\": [], \"summary\": \"clean\"}\n```";

    let value = match extract_json(clean).expect("parses") {
        Extracted::Complete(value) => value,
        other => panic!("expected Complete, got {other:?}"),
    };

    assert_eq!(value, json!({ "issues": [], "summary": "clean" }));
}

#[test]
fn whitespace_before_the_opening_tag_is_tolerated() {
    let input = "\n  <think>brief</think>\n{\"issues\": []}";

    let value = match extract_json(input).expect("parses") {
        Extracted::Complete(value) => value,
        other => panic!("expected Complete, got {other:?}"),
    };

    assert_eq!(value["issues"], json!([]));
}

#[test]
fn a_stripped_response_with_nothing_after_it_is_unparseable() {
    // The model spent its whole budget deliberating. There is no answer to
    // report, and inventing an empty one would report the file as clean.
    assert_eq!(extract_json("<think>all budget spent</think>"), None);
}

#[test]
fn truncation_recovery_still_applies_after_a_strip() {
    // The strip must not short-circuit the rest of the ladder: a response cut
    // off mid-answer is still recoverable as a prefix, and still `Truncated`.
    let input = "<think>reasoning</think>\n{\"issues\": [{\"line\": 1,";

    match extract_json(input) {
        Some(Extracted::Truncated(value)) => assert_eq!(value["issues"][0]["line"], json!(1)),
        other => panic!("expected Truncated, got {other:?}"),
    }
}

#[test]
fn a_json_response_quoting_a_code_fence_is_not_mangled_by_it() {
    // drep's own pre-push gate hit this reviewing `json_parsing.rs`: the model
    // answered with valid JSON whose finding *described* fence handling, so the
    // message contained "```". The fence strategy matched that inner fence,
    // took its body, and every later strategy then ran on prose - reporting a
    // perfectly good answer as unparseable, and failing the push.
    let body = concat!(
        r#"{"issues": [{"line": 86, "severity": "high", "category": "bug", "#,
        r#""message": "a fenced block like ```json {\"a\": 1} ``` inside a string"}], "#,
        r#""summary": "one issue"}"#
    );

    let value = match extract_json(body).expect("valid JSON is the answer") {
        Extracted::Complete(value) => value,
        other => panic!("expected Complete, got {other:?}"),
    };

    assert_eq!(value["summary"], json!("one issue"));
    assert!(
        value["issues"][0]["message"]
            .as_str()
            .expect("message")
            .contains("```json"),
        "the fence inside the string survives untouched: {value}"
    );
}

#[test]
fn a_fenced_response_still_wins_when_the_content_itself_is_not_json() {
    // The case strategy 1 must not break: prose around a fenced answer. Trying
    // the whole content first has to fail here and fall through to the fence.
    let body = "Here is my review:\n```json\n{\"issues\": [], \"summary\": \"clean\"}\n```\nHope that helps.";

    let value = match extract_json(body).expect("the fence carries the answer") {
        Extracted::Complete(value) => value,
        other => panic!("expected Complete, got {other:?}"),
    };

    assert_eq!(value["summary"], json!("clean"));
}

#[test]
fn a_reasoning_block_is_still_stripped_before_the_direct_parse() {
    // Strategy 0 runs first, so a `<think>` block cannot make the whole-content
    // parse fail and push a valid answer down the ladder.
    let body = "<think>deliberating</think>{\"issues\": [], \"summary\": \"clean\"}";

    let value = match extract_json(body).expect("parses") {
        Extracted::Complete(value) => value,
        other => panic!("expected Complete, got {other:?}"),
    };

    assert_eq!(value["summary"], json!("clean"));
}
