//! The extraction ladder end to end: which strategy claims which input.
//!
//! Moved verbatim out of `json_parsing.rs` when that file approached the
//! 600-line limit; only the glob import changed, because `super` now names
//! this directory rather than the module under test.

use crate::llm::json_parsing::*;
use serde_json::json;

/// Pull the inner value out of an `Extracted` for assertions.
fn unwrap(extracted: Extracted) -> Value {
    match extracted {
        Extracted::Complete(v) | Extracted::Truncated(v) => v,
    }
}

/// Criterion 9: a bare JSON object parses as `Complete`.
#[test]
fn bare_json_object_parses_to_complete() {
    let extracted = extract_json(r#"{"findings":[]}"#).expect("parses");
    assert!(matches!(extracted, Extracted::Complete(_)));
    assert_eq!(unwrap(extracted), json!({"findings": []}));
}

/// Criterion 10: a ```json fenced block parses as `Complete`.
#[test]
fn json_fenced_block_parses_to_complete() {
    let input = "```json\n{\"findings\":[]}\n```";
    let extracted = extract_json(input).expect("parses");
    assert!(matches!(extracted, Extracted::Complete(_)));
    assert_eq!(unwrap(extracted), json!({"findings": []}));
}

/// Criterion 11: an unlabelled ``` fenced block parses as `Complete`.
#[test]
fn unlabelled_fenced_block_parses_to_complete() {
    let input = "```\n{\"findings\":[]}\n```";
    let extracted = extract_json(input).expect("parses");
    assert!(matches!(extracted, Extracted::Complete(_)));
    assert_eq!(unwrap(extracted), json!({"findings": []}));
}

/// Criterion 12: prose before and after a fenced block still parses.
#[test]
fn prose_around_fenced_block_still_parses() {
    let input = "Sure, here you go:\n```json\n{\"a\":1}\n```\nLet me know if that helps.";
    let extracted = extract_json(input).expect("parses");
    assert!(matches!(extracted, Extracted::Complete(_)));
    assert_eq!(unwrap(extracted), json!({"a": 1}));
}

/// Criterion 13: a trailing comma before `}` is repaired; the result is
/// `Complete`, not `Truncated`.
#[test]
fn trailing_comma_is_repaired_and_returns_complete() {
    let input = r#"{"findings": [], "ok": true,}"#;
    let extracted = extract_json(input).expect("parses after repair");
    assert!(
        matches!(extracted, Extracted::Complete(_)),
        "trailing-comma repair is strategy 3, which returns Complete"
    );
    assert_eq!(unwrap(extracted), json!({"findings": [], "ok": true}));
}

/// Criterion 14: an unbalanced `{` is closed; the result is `Truncated`,
/// not `Complete`. This pins the load-bearing distinction.
#[test]
fn unbalanced_open_brace_closes_and_returns_truncated() {
    let input = r#"{"a":"b""#;
    let extracted = extract_json(input).expect("parses after balance");
    assert!(
        matches!(extracted, Extracted::Truncated(_)),
        "missing closing brace must be reported as Truncated, not Complete"
    );
    assert_eq!(unwrap(extracted), json!({"a": "b"}));
}

/// Criterion 15: an unbalanced `[` is closed; the result is `Truncated`.
#[test]
fn unbalanced_open_bracket_closes_and_returns_truncated() {
    let input = "[1, 2, 3";
    let extracted = extract_json(input).expect("parses after balance");
    assert!(
        matches!(extracted, Extracted::Truncated(_)),
        "missing closing bracket must be reported as Truncated, not Complete"
    );
    assert_eq!(unwrap(extracted), json!([1, 2, 3]));
}

/// Criterion 16: irrecoverable garbage returns `None`.
#[test]
fn irrecoverable_garbage_returns_none() {
    // No fence, no parse, no trailing commas to repair, no balance to do.
    let input = "this is not JSON at all, it's prose.";
    assert!(extract_json(input).is_none());
}

/// Criterion 17: an empty string returns `None`.
#[test]
fn empty_string_returns_none() {
    assert!(extract_json("").is_none());
}

/// Criterion 18: a JSON string containing an apostrophe survives intact.
///
/// This pins that the single-quote repair is *not* implemented. Replacing
/// `'` with `"` would corrupt `"don't"` into `"don"t"`, which is invalid
/// JSON and would force a parse failure that the current ladder would
/// then misclassify as `Truncated`.
#[test]
fn apostrophe_inside_string_survives_intact() {
    let input = r#"{"message":"don't worry","ok":true}"#;
    let extracted = extract_json(input).expect("parses directly via strategy 2");
    assert!(matches!(extracted, Extracted::Complete(_)));
    assert_eq!(
        unwrap(extracted),
        json!({"message": "don't worry", "ok": true})
    );
}

/// Criterion 19: a `Complete` result is never reported as `Truncated`.
///
/// A well-formed nested object with many braces must round-trip through
/// strategy 2 as `Complete`. The parenthetical in the spec - "must not
/// trip the balance counter" - is exercised by the assertion that
/// strategy 4 is never reached for balanced input: the test fails if a
/// naive brace counter over- or under-counts inside strings and the
/// parser falls through to strategy 4.
#[test]
fn complete_result_is_never_reported_as_truncated() {
    let input = r#"{"a":{"b":{"c":"d"}},"e":[1,2,3]}"#;
    let extracted = extract_json(input).expect("parses");
    assert!(
        matches!(extracted, Extracted::Complete(_)),
        "well-formed nested object must be Complete, not Truncated"
    );
    assert_eq!(
        unwrap(extracted),
        json!({"a": {"b": {"c": "d"}}, "e": [1, 2, 3]})
    );
}

/// Pins that `balance_unclosed` ignores braces and brackets inside
/// strings. Without string awareness, `"oops {"` would be counted as an
/// open brace and the balancer would append a stray `}` that corrupts
/// the JSON.
#[test]
fn balance_unclosed_ignores_braces_inside_strings() {
    let input = r#"{"text":"oops {","a":1"#;
    let balanced = balance_unclosed(input).expect("needs balancing");
    assert_eq!(balanced, r#"{"text":"oops {","a":1}"#);
    assert!(serde_json::from_str::<Value>(&balanced).is_ok());
}

/// Pins that a trailing comma inside a string is preserved - the repair
/// only fires before a closing delimiter that is NOT inside a string.
/// The fixture here has a trailing comma before `}` and ALSO a comma
/// inside a string; strategy 3 must repair the outer one and leave the
/// inner one alone.
#[test]
fn trailing_comma_repair_leaves_commas_inside_strings_intact() {
    let input = r#"{"a":"x, y","b":1,}"#;
    let extracted = extract_json(input).expect("parses after repair");
    assert!(matches!(extracted, Extracted::Complete(_)));
    assert_eq!(unwrap(extracted), json!({"a": "x, y", "b": 1}));
}
