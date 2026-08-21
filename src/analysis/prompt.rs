//! The code-quality system prompt for one language.
//!
//! The prompt is the bridge between the deterministic layer (the configured
//! linters and formatters) and the semantic layer (the LLM): it tells the
//! model what drep has already done and what is left for it to do. The body
//! is identical for every language; only `display_name` and `conventions`
//! vary, so the JSON shape the model is asked to return is never language-
//! dependent and the parser in [`crate::analysis::code_quality`] stays total.
//!
//! Two instructions are load-bearing and must survive untouched:
//!
//! - The "do not report anything a formatter or linter would catch" line.
//!   The deterministic layer already produces those findings, and without
//!   this guard the model floods every file with lint noise that the gate
//!   then has to deduplicate against the tooling's own output.
//! - The schema block. A model that emits a different shape is, from the
//!   parser's point of view, an unanalyzed file — the parser does not
//!   attempt to recover a foreign schema.
//!
//! Every payload line carries its real file line number in the gutter, and a
//! finding must not be reported on a line that has no number. Together with
//! [`crate::analysis::payload::Payload::valid_lines`] they are the line-
//! number provenance the parser relies on to drop findings that point at
//! code the model was never shown.

use crate::analysis::findings::LlmSeverity;
use crate::analysis::response_contract::{
    CATEGORY, CODE_SNIPPET, COMPILE_FAILURE, ISSUES, LINE, MESSAGE, SEVERITY, SUGGESTION, SUMMARY,
};
use crate::languages::spec::LanguageSupport;

/// Build the code-quality system prompt for one language.
///
/// The whole body is one template: the categories, the JSON schema, and the
/// "do not duplicate the linter" instruction are fixed across languages, so
/// the model is always answering the same question in the same shape. Only
/// `display_name` and the optional `conventions` block move.
///
/// Pass `&LanguageSupport` rather than `&str` so the registry key
/// (`"rust"`) cannot be substituted for the model-facing name (`"Rust"`).
pub fn build_analysis_prompt(language: &LanguageSupport) -> String {
    let conventions = conventions_block(language);
    let display_name = language.display_name;
    // Rendered from the same review vocabulary as the strict output schema.
    // The parser accepts the wider legacy vocabulary so an old cache entry or
    // unconstrained provider response cannot make a whole file malformed.
    let severities = LlmSeverity::review_alternation();
    let issues = ISSUES;
    let summary = SUMMARY;
    let line = LINE;
    let severity = SEVERITY;
    let category = CATEGORY;
    let message = MESSAGE;
    let suggestion = SUGGESTION;
    let code_snippet = CODE_SNIPPET;
    let compile_failure = COMPILE_FAILURE;
    // The template is shaped so the conventions block, when empty, leaves
    // no stray heading and no doubled blank line. The newline after the
    // placeholder is the only one that exists in the template, so an
    // empty conventions block is followed directly by `For each issue
    // found`.
    format!(
        "You are an expert {display_name} code reviewer.\n\
         Review the following code as a merge gate. Report only concrete issues\n\
         that are worth fixing before merge:\n\
         \n\
         1. **Bugs & Logic Errors**: Incorrect logic, reachable crashes, data\n\
            loss, broken contracts, type errors\n\
         2. **Security Issues**: Injection, path traversal, unsafe deserialization,\n\
            hardcoded secrets, weak cryptography\n\
         3. **Reliability & Maintainability Defects**: Resource leaks, races,\n\
            inconsistent state, or a design defect with a concrete failure mode\n\
         4. **Performance Defects**: Material algorithmic or resource problems on\n\
            a plausible execution path\n\
         \n\
         {conventions}\
         For each issue found, provide:\n\
         - The exact gutter line number of the affected code\n\
         - Severity: critical (security vulnerabilities, data loss), high (bugs,\n\
           crashes, serious issues), medium (material but non-critical defects).\n\
           Low and info suggestions are outside this review and must not be emitted\n\
         - Category: bug, security, performance, maintainability\n\
         - Clear message explaining the issue\n\
         - Specific, actionable suggestion for fixing it\n\
         - The problematic code snippet\n\
         - Whether the finding explicitly claims the code cannot compile\n\
         \n\
         **Important instructions:**\n\
         - Only report a finding when it is concrete and reachable from the code\n\
           shown, with a plausible execution path and a material consequence\n\
         - This is not an exhaustive hardening exercise. Do not report optional hardening,\n\
           extreme edge cases without a plausible execution path, nits, subjective\n\
           preferences, cleanup, or refactoring opportunities\n\
         - Do not report missing tests or documentation unless their absence creates\n\
           a concrete product or API defect in the shown change\n\
         - Prefer no finding over a speculative or marginal finding\n\
         - Provide actionable suggestions, not vague advice\n\
         - Focus on correctness, security, reliability, and material performance\n\
         - The input is a line-numbered excerpt. Report the finding's `line`\n\
           as the number shown in the gutter, never an offset into the\n\
           excerpt. The excerpt itself states which lines are in scope.\n\
         - Do not report subjective style issues, and do not report anything a\n\
           formatter or linter would catch: those run separately and deterministically\n\
         \n\
         Return your analysis as valid JSON matching this exact schema:\n\
         {{\n\
           \"{issues}\": [\n\
             {{\n\
               \"{line}\": <line_number>,\n\
               \"{severity}\": \"<{severities}>\",\n\
               \"{category}\": \"<bug|security|performance|maintainability>\",\n\
               \"{message}\": \"<clear description of the issue>\",\n\
               \"{suggestion}\": \"<specific recommendation for fixing>\",\n\
               \"{code_snippet}\": \"<the problematic code>\",\n\
               \"{compile_failure}\": <true|false>\n\
             }}\n\
           ],\n\
           \"{summary}\": \"<overall assessment of code quality>\"\n\
         }}\n\
         \n\
         If no issues are found, return:\n\
         {{\n\
           \"{issues}\": [],\n\
           \"{summary}\": \"No significant issues found. Code quality looks good.\"\n\
         }}\n"
    )
}

/// Build the language-specific concerns block, or an empty string when the
/// language has no conventions.
///
/// Heading + one bullet per entry, trailing newline. The newline is consumed
/// by the template's own following newline so a missing block leaves no
/// blank hole and a present block does not leave a doubled one.
fn conventions_block(language: &LanguageSupport) -> String {
    if language.conventions.is_empty() {
        return String::new();
    }
    let mut out = format!("**{}-specific concerns:**\n", language.display_name);
    for concern in language.conventions {
        out.push_str("- ");
        out.push_str(concern);
        out.push('\n');
    }
    out
}
