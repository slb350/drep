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

/// The opening and closing tags of an inline reasoning block.
const THINK_OPEN: &str = "<think>";
const THINK_CLOSE: &str = "</think>";

/// Drop a leading `<think>...</think>` block, returning what follows it.
///
/// Reasoning models are supposed to stream deliberation on a side channel -
/// `reasoning_content` on DeepSeek and z.ai, `thinking` blocks on Anthropic -
/// which the SDK routes away from content before drep ever sees it. Several
/// OpenAI-compatible servers do not: MiniMax's M-series and most local
/// llama.cpp and MLX builds of Qwen emit the whole trace inline at the head of
/// `message.content`, wrapped in these tags.
///
/// That breaks the ladder rather than merely adding noise. Deliberation about
/// a code review quotes code, so the reasoning carries fenced blocks of its
/// own; `FENCE_RE` takes the *first* fence in the text, so the ladder selected
/// the reasoning's sample, strategies 2 through 4 all failed on it, and the
/// file came back `Unparseable` - which by design neither fails over nor
/// retries, so the configured fallback was never reached and every file failed.
///
/// Three properties, each load-bearing:
///
/// - **Anchored at the start.** A `<think>` appearing anywhere else is content -
///   a finding about a file that contains the tag, most obviously - and
///   stripping it would rewrite the model's answer.
/// - **A closing tag is required.** Without one the block never ended, so the
///   answer never arrived; `Unparseable` is then the honest outcome and
///   swallowing the rest of the response would hide it.
/// - **Leading whitespace is tolerated** before the opening tag but the tag
///   itself must come first, because servers differ on whether they emit a
///   newline ahead of it.
fn strip_reasoning_block(content: &str) -> &str {
    let Some(rest) = content.trim_start().strip_prefix(THINK_OPEN) else {
        return content;
    };
    let Some(close) = rest.find(THINK_CLOSE) else {
        return content;
    };
    &rest[close + THINK_CLOSE.len()..]
}

/// Strip a fenced JSON block out of `content`, returning the body.
///
/// Returns `None` if there is no fence. Trims surrounding whitespace from the
/// body so callers can hand it to `serde_json` without further scrubbing.
static FENCE_RE: LazyLock<Regex> = LazyLock::new(|| {
    // ``` (literal) + optional `json` info string + newline + body + newline + ```
    // Non-greedy body so a second fence in the same text doesn't swallow
    // everything in between.
    // (?s) so `.` crosses newlines. Without it only a single-line body matches,
    // and real model output is pretty-printed - every fenced multi-line response
    // fell through to `None` and the file was reported unanalyzed.
    // Also tolerate CRLF, trailing spaces after the info string, and an indented
    // closing fence, all of which real responses produce.
    Regex::new(r"(?s)```(?:json)?[ \t]*\r?\n(.*?)\r?\n[ \t]*```")
        .expect("FENCE_RE is a constant regex")
});

/// Match a trailing comma before `}` or `]`, capturing the closing delimiter
/// so `replace_all` can drop just the comma.
/// Drop commas that sit immediately before a closing `}` or `]`, ignoring any
/// that appear inside a JSON string.
///
/// This was a regex (`,(\s*[}\]])`) applied to the raw text, which had no idea
/// what a string was: `{"a":",}","b":1,}` was "repaired" into `{"a":"}","b":1}`,
/// parsing as `Complete` with the string value silently rewritten from `,}` to
/// `}`. That is the same defect class as the single-quote repair this module
/// deliberately does not implement - a repair that damages valid input.
fn strip_trailing_commas(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut in_string = false;
    let mut escape = false;

    for (i, ch) in s.char_indices() {
        if escape {
            escape = false;
            out.push(ch);
            continue;
        }
        if in_string {
            match ch {
                '\\' => escape = true,
                '"' => in_string = false,
                _ => {}
            }
            out.push(ch);
            continue;
        }
        if ch == '"' {
            in_string = true;
            out.push(ch);
            continue;
        }
        if ch == ',' {
            // Look ahead past whitespace for a closing delimiter. Only a comma
            // outside a string can be structural, which is why this check lives
            // here rather than in a regex over the whole text.
            let rest = &bytes[i + 1..];
            let next = rest.iter().find(|b| !b.is_ascii_whitespace());
            if matches!(next, Some(b'}') | Some(b']')) {
                continue;
            }
        }
        out.push(ch);
    }
    out
}

/// Run the strategy ladder against one LLM response and return the first thing
/// that parses.
///
/// Returns `None` when every strategy fails. An empty input also returns
/// `None` - there is nothing to parse.
pub fn extract_json(content: &str) -> Option<Extracted> {
    // Strategy 0: drop a leading reasoning block. Must run before the fence
    // ladder, because the reasoning routinely contains a fence of its own and
    // `FENCE_RE` takes the *first* one.
    let content = strip_reasoning_block(content);

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
    let repaired = strip_trailing_commas(&working);
    if let Ok(value) = serde_json::from_str::<Value>(&repaired) {
        return Some(Extracted::Complete(value));
    }

    // Strategy 4: truncation recovery. Count open vs. close braces/brackets
    // outside of strings, append the missing closers, and parse. Returning
    // `Truncated` is the load-bearing signal: a successful parse here means
    // the response was cut off and the value is a prefix of the intended
    // document.
    // Balance first, then strip: a truncated response very often ends mid-element
    // (`{"a":1,`), where the dangling comma has no closing delimiter after it yet
    // and so is invisible to the stripper. Appending the closers first turns it
    // into `{"a":1,}`, which the stripper then repairs. Doing it the other way
    // round leaves the comma in place and the parse fails.
    if let Some(balanced) = balance_unclosed(&repaired).map(|b| strip_trailing_commas(&b))
        && let Ok(value) = serde_json::from_str::<Value>(&balanced)
    {
        return Some(Extracted::Truncated(value));
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
mod tests;
