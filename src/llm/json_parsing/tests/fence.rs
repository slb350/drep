//! Fence extraction, including the CRLF and indented-close tolerances.
//!
//! Moved verbatim out of `json_parsing.rs` when that file approached the
//! 600-line limit; only the glob import changed, because `super` now names
//! this directory rather than the module under test.

use crate::llm::json_parsing::*;

/// The realistic shape: a pretty-printed body inside a fence.
///
/// The original regex had no `(?s)`, so `.` never crossed a newline and this
/// returned `None`. The single-line fenced tests all passed regardless,
/// which is why the gap survived review.
#[test]
fn a_multi_line_fenced_block_parses() {
    let content =
        "Here is the review:\n```json\n{\n  \"findings\": [\n    {\"line\": 1}\n  ]\n}\n```\ndone";
    match extract_json(content) {
        Some(Extracted::Complete(v)) => assert_eq!(v["findings"][0]["line"], 1),
        other => panic!("expected Complete, got {other:?}"),
    }
}

#[test]
fn crlf_line_endings_in_a_fence_parse() {
    let content = "```json\r\n{\r\n  \"a\": 1\r\n}\r\n```";
    match extract_json(content) {
        Some(Extracted::Complete(v)) => assert_eq!(v["a"], 1),
        other => panic!("expected Complete, got {other:?}"),
    }
}

#[test]
fn an_indented_closing_fence_parses() {
    let content = "```json\n{\n  \"a\": 1\n}\n  ```";
    match extract_json(content) {
        Some(Extracted::Complete(v)) => assert_eq!(v["a"], 1),
        other => panic!("expected Complete, got {other:?}"),
    }
}
