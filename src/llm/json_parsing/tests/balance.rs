//! Truncation recovery: counting unclosed delimiters outside strings.
//!
//! Moved verbatim out of `json_parsing.rs` when that file approached the
//! 600-line limit; only the glob import changed, because `super` now names
//! this directory rather than the module under test.

use crate::llm::json_parsing::*;

fn value_of(extracted: Option<Extracted>) -> Value {
    match extracted {
        Some(Extracted::Truncated(v)) => v,
        other => panic!("expected Truncated, got {other:?}"),
    }
}

/// Nesting depth must be *counted*, not merely detected.
///
/// One missing brace and three missing braces need different repairs.
/// Mutating the `+=` on the open-brace counter to `-=` or `*=` produces the
/// wrong count, which the single-level cases cannot see because one is the
/// same under almost any arithmetic.
#[test]
fn three_unclosed_braces_need_three_closers() {
    let v = value_of(extract_json(r#"{"a":{"b":{"c":1"#));
    assert_eq!(v["a"]["b"]["c"], 1, "all three levels must survive");
}

#[test]
fn nested_unclosed_brackets_need_matching_closers() {
    let v = value_of(extract_json(r#"{"xs":[[1,2],[3"#));
    assert_eq!(v["xs"][0][0], 1);
    assert_eq!(
        v["xs"][1][0], 3,
        "the inner array must close before the outer"
    );
}

#[test]
fn braces_and_brackets_are_counted_separately() {
    let v = value_of(extract_json(r#"{"xs":[{"k":1"#));
    assert_eq!(v["xs"][0]["k"], 1);
}

/// A brace inside a string is text, not structure.
///
/// Without the in-string guard the `{` here is counted as nesting and the
/// repair appends a spurious closer, which fails to parse.
#[test]
fn braces_inside_strings_are_not_structural() {
    let v = value_of(extract_json(r#"{"msg":"a { brace","n":1"#));
    assert_eq!(v["msg"], "a { brace");
    assert_eq!(v["n"], 1);
}

/// An escaped quote does not end the string, so delimiters after it are
/// still text.
///
/// Deleting the `'\\' => escape = true` arm makes the parser treat `\\"` as a
/// closing quote, after which the `[[[` inside the message are counted as
/// three open arrays. The repair then appends `]]]` and the result does not
/// parse. The brackets are what make this discriminating: a case with only
/// braces can coincidentally rebalance.
#[test]
fn escaped_quotes_do_not_end_the_string() {
    let v = value_of(extract_json(r#"{"m":"x\"[[[","n":1"#));
    assert_eq!(v["m"], r#"x"[[["#);
    assert_eq!(v["n"], 1);
}

/// A trailing backslash-escaped backslash is a complete escape, so the
/// following quote genuinely closes the string.
#[test]
fn an_escaped_backslash_does_not_swallow_the_closing_quote() {
    let v = value_of(extract_json(r#"{"path":"C:\\","n":2"#));
    assert_eq!(v["path"], r"C:\");
    assert_eq!(v["n"], 2);
}
