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
//! 2.x advertises two facts the Python did not need to: every line in the
//! payload carries a real file line number in the gutter, and a finding must
//! not be reported on a line that has no number. Together with
//! [`crate::analysis::payload::Payload::valid_lines`] they are the line-
//! number provenance the parser relies on to drop findings that point at
//! code the model was never shown.

use crate::analysis::findings::LlmSeverity;
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
    // Rendered from `LlmSeverity::ALL`, so the levels the prompt asks for and
    // the levels the parser accepts are the same list by construction.
    let severities = LlmSeverity::alternation();
    // The template is shaped so the conventions block, when empty, leaves
    // no stray heading and no doubled blank line. The newline after the
    // placeholder is the only one that exists in the template, so an
    // empty conventions block is followed directly by `For each issue
    // found`.
    format!(
        "You are an expert {display_name} code reviewer.\n\
         Analyze the following code and identify issues in these categories:\n\
         \n\
         1. **Bugs & Logic Errors**: Incorrect logic, unhandled edge cases,\n\
            potential crashes, undefined variables, type errors\n\
         2. **Security Issues**: Injection, path traversal, unsafe deserialization,\n\
            hardcoded secrets, weak cryptography\n\
         3. **Best Practices**: Poor naming, code smells, anti-patterns\n\
         4. **Performance**: Inefficient algorithms, unnecessary work,\n\
            blocking I/O, memory leaks\n\
         \n\
         {conventions}\
         For each issue found, provide:\n\
         - Line number (approximate if exact line is unclear)\n\
         - Severity: critical (security vulnerabilities, crashes), high (bugs,\n\
           serious issues), medium (best practices, moderate issues), low (minor\n\
           improvements), info (suggestions)\n\
         - Category: bug, security, best-practice, performance, style, maintainability\n\
         - Clear message explaining the issue\n\
         - Specific, actionable suggestion for fixing it\n\
         - The problematic code snippet\n\
         \n\
         **Important instructions:**\n\
         - Only report genuine issues, not false positives\n\
         - Be specific about line numbers - estimate if needed\n\
         - Provide actionable suggestions, not vague advice\n\
         - Focus on correctness, security, and maintainability\n\
         - The input is a line-numbered excerpt. Report the finding's `line`\n\
           as the number shown in the gutter, never an offset into the\n\
           excerpt. The excerpt itself states which lines are in scope.\n\
         - Do not report subjective style issues, and do not report anything a\n\
           formatter or linter would catch: those run separately and deterministically\n\
         \n\
         Return your analysis as valid JSON matching this exact schema:\n\
         {{\n\
           \"issues\": [\n\
             {{\n\
               \"line\": <line_number>,\n\
               \"severity\": \"<{severities}>\",\n\
               \"category\": \"<bug|security|best-practice|performance|style|maintainability>\",\n\
               \"message\": \"<clear description of the issue>\",\n\
               \"suggestion\": \"<specific recommendation for fixing>\",\n\
               \"code_snippet\": \"<the problematic code>\"\n\
             }}\n\
           ],\n\
           \"summary\": \"<overall assessment of code quality>\"\n\
         }}\n\
         \n\
         If no issues are found, return:\n\
         {{\n\
           \"issues\": [],\n\
           \"summary\": \"No significant issues found. Code quality looks good.\"\n\
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
