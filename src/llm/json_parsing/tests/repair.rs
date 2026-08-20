//! Trailing-comma repair, and the strings it must not touch.
//!
//! Moved verbatim out of `json_parsing.rs` when that file approached the
//! 600-line limit; only the glob import changed, because `super` now names
//! this directory rather than the module under test.

use crate::llm::json_parsing::*;

/// A comma inside a string is data, not syntax.
///
/// The regex this replaced rewrote `{"a":",}","b":1,}` into
/// `{"a":"}","b":1}` and returned `Complete` - a silent corruption of a
/// string value, in a field that carries finding messages.
#[test]
fn a_comma_inside_a_string_is_not_stripped() {
    match extract_json(r#"{"a":",}","b":1,}"#) {
        Some(Extracted::Complete(v)) => {
            assert_eq!(v["a"], ",}", "the string value must survive intact");
            assert_eq!(v["b"], 1);
        }
        other => panic!("expected Complete, got {other:?}"),
    }
}

#[test]
fn a_structural_trailing_comma_is_still_stripped() {
    match extract_json(r#"{"a":1,}"#) {
        Some(Extracted::Complete(v)) => assert_eq!(v["a"], 1),
        other => panic!("expected Complete, got {other:?}"),
    }
}

#[test]
fn an_escaped_quote_does_not_expose_a_comma_to_the_stripper() {
    match extract_json(r#"{"a":"x\",}","b":2,}"#) {
        Some(Extracted::Complete(v)) => {
            assert_eq!(v["a"], r#"x",}"#);
            assert_eq!(v["b"], 2);
        }
        other => panic!("expected Complete, got {other:?}"),
    }
}

/// Repairs must compose: truncated *and* trailing-comma is the common shape
/// of a response cut off mid-element.
#[test]
fn a_truncated_object_with_a_trailing_comma_recovers() {
    match extract_json(r#"{"a":1,"#) {
        Some(Extracted::Truncated(v)) => assert_eq!(v["a"], 1),
        other => panic!("expected Truncated, got {other:?}"),
    }
}
