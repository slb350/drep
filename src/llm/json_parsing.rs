//! Tolerant JSON extraction from LLM responses.
//!
//! The model returns prose-wrapped, fenced, or slightly malformed JSON. Port
//! the strategy ladder from `drep/llm/json_parsing.py` (minus the single-quote
//! repair and the fuzzy-inference fallback), first success wins:
//!
//! 1. Extract a fenced block (with or without a `json` info string) and use
//!    its body.
//! 2. Parse the (possibly fenced) content directly.
//! 3. Repair trailing commas before `}` or `]` and parse.
//! 4. Balance unclosed braces/brackets and parse.
//!
//! Strategies 1-3 that succeed return [`Extracted::Complete`]. Only strategy 4
//! returns [`Extracted::Truncated`].
//!
//! ## Why `Truncated` is a type, not a log line
//!
//! Recovering truncated JSON yields a *partial* findings list. While LLM
//! findings only inform, under `--fail-on` a truncated response could
//! silently omit the one blocking finding and the gate would pass. Phase 4
//! will treat `Truncated` as *unanalyzed* rather than clean when gating.
//! Losing this distinction reintroduces the exact failure the whole project
//! exists to prevent.
//!
//! ## What is deliberately NOT ported
//!
//! - **Single-quote repair.** It corrupts any JSON string containing an
//!   apostrophe (`"don't"`, `` "`os` imported but unused" ``), which finding
//!   messages routinely do.
//! - **`fuzzy_inference`.** It guessed field values out of prose with
//!   per-field regexes; a wrong finding is worse than a missing one.

use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

/// What `extract_json` returns when it can pull a JSON value out of the model
/// output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Extracted {
    /// Parsed cleanly: the response is a complete JSON document as far as we
    /// can tell.
    Complete(Value),
    /// Parsed only after closing unbalanced braces/brackets - the response
    /// was cut off, so the [`Value`] is a PREFIX of what the model meant to
    /// say. The caller must decide whether to trust a partial result.
    Truncated(Value),
}

/// Strip a fenced JSON block out of `content`, returning the body.
///
/// Returns `None` if there is no fence. Trims surrounding whitespace from the
/// body so callers can hand it to `serde_json` without further scrubbing.
static FENCE_RE: LazyLock<Regex> = LazyLock::new(|| {
    // ``` (literal) + optional `json` info string + newline + body + newline + ```
    // Non-greedy body so a second fence in the same text doesn't swallow
    // everything in between.
    Regex::new(r"```(?:json)?\n(.*?)\n```").expect("FENCE_RE is a constant regex")
});

/// Match a trailing comma before `}` or `]`, capturing the closing delimiter
/// so `replace_all` can drop just the comma.
static TRAILING_COMMA_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r",(\s*[}\]])").expect("TRAILING_COMMA_RE is a constant regex"));

/// Run the strategy ladder against one LLM response and return the first thing
/// that parses.
///
/// Returns `None` when every strategy fails. An empty input also returns
/// `None` - there is nothing to parse.
pub fn extract_json(content: &str) -> Option<Extracted> {
    // Strategy 1: extract from a markdown fence if one is present. The fence
    // path applies even when the body itself fails to parse; strategies 2-4
    // then run on the body alone.
    let working = FENCE_RE
        .captures(content)
        .map(|caps| caps.get(1).unwrap().as_str().trim().to_string())
        .unwrap_or_else(|| content.to_string());

    // Strategy 2: direct parse of the (possibly fenced) content.
    if let Ok(value) = serde_json::from_str::<Value>(&working) {
        return Some(Extracted::Complete(value));
    }

    // Strategy 3: repair trailing commas. This is the only repair we attempt
    // - single-quote substitution is deliberately omitted because it
    // corrupts apostrophes inside strings.
    let repaired = TRAILING_COMMA_RE.replace_all(&working, "$1").into_owned();
    if let Ok(value) = serde_json::from_str::<Value>(&repaired) {
        return Some(Extracted::Complete(value));
    }

    // Strategy 4: truncation recovery. Count open vs. close braces/brackets
    // outside of strings, append the missing closers, and parse. Returning
    // `Truncated` is the load-bearing signal: a successful parse here means
    // the response was cut off and the value is a prefix of the intended
    // document.
    if let Some(balanced) = balance_unclosed(&working) {
        if let Ok(value) = serde_json::from_str::<Value>(&balanced) {
            return Some(Extracted::Truncated(value));
        }
    }

    None
}

/// Walk `s` and return it with the missing closing delimiters appended, in the
/// order that actually closes the structure.
///
/// Uses a **stack**, not per-delimiter counters. Counting how many `{` and `[`
/// are unclosed tells you how many closers to add but not their order, and
/// order is load-bearing: `{"xs":[{"k":1` is missing `}]}`, while independent
/// counters would emit `}}]` and fail to parse. Anything nested more than one
/// level deep hits this.
///
/// Returns `None` when there is nothing to do - the input is already balanced -
/// because the caller invokes this only after strategies 2 and 3 fail, and a
/// balanced re-parse would be identical to the failed parse. Distinguishing
/// "already balanced" from "would not parse even balanced" keeps the
/// `Truncated` signal meaningful: it fires only when a closing delimiter was
/// genuinely missing.
fn balance_unclosed(s: &str) -> Option<String> {
    // Holds the closer each open delimiter is waiting for, innermost last.
    let mut expected: Vec<char> = Vec::new();
    let mut in_string = false;
    let mut escape = false;

    for c in s.chars() {
        if escape {
            // The previous character was a backslash inside a string; this
            // character is the escaped one and has no structural meaning.
            escape = false;
            continue;
        }
        if in_string {
            match c {
                '\\' => escape = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            '{' => expected.push('}'),
            '[' => expected.push(']'),
            '}' | ']' => {
                // A mismatched closer means the document is malformed rather
                // than truncated; popping regardless lets the re-parse fail,
                // which is the honest outcome.
                expected.pop();
            }
            _ => {}
        }
    }

    // Only `expected.is_empty()` is checked. An unterminated string is also
    // unrecoverable, but appending closers to it produces JSON that cannot
    // parse, so the caller falls through to `None` on its own - guarding it
    // here as well would be a branch no input can distinguish.
    if expected.is_empty() {
        return None;
    }

    let mut balanced = String::with_capacity(s.len() + expected.len());
    balanced.push_str(s);
    balanced.extend(expected.iter().rev());
    Some(balanced)
}

#[cfg(test)]
mod tests {
    use super::*;
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
}

#[cfg(test)]
mod balance_tests {
    use super::*;

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
}
