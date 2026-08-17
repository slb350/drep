"""Shared analysis-prompt scaffolding.

One prompt body for every language: the categories, the instructions and - most
importantly - the JSON schema are identical, so `CodeAnalysisResult` parsing
never has to know which language produced a response. Only the language name
and its conventions vary.
"""

from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from drep.languages.base import LanguageSupport

_PROMPT_TEMPLATE = """You are an expert {display_name} code reviewer.
Analyze the following code and identify issues in these categories:

1. **Bugs & Logic Errors**: Incorrect logic, unhandled edge cases,
   potential crashes, undefined variables, type errors
2. **Security Issues**: Injection, path traversal, unsafe deserialization,
   hardcoded secrets, weak cryptography
3. **Best Practices**: Poor naming, code smells, anti-patterns
4. **Performance**: Inefficient algorithms, unnecessary work,
   blocking I/O, memory leaks

{conventions}
For each issue found, provide:
- Line number (approximate if exact line is unclear)
- Severity: critical (security vulnerabilities, crashes), high (bugs,
  serious issues), medium (best practices, moderate issues), low (minor
  improvements), info (suggestions)
- Category: bug, security, best-practice, performance, style, maintainability
- Clear message explaining the issue
- Specific, actionable suggestion for fixing it
- The problematic code snippet

**Important instructions:**
- Only report genuine issues, not false positives
- Be specific about line numbers - estimate if needed
- Provide actionable suggestions, not vague advice
- Focus on correctness, security, and maintainability
- Do not report subjective style issues, and do not report anything a
  formatter or linter would catch: those run separately and deterministically

Return your analysis as valid JSON matching this exact schema:
{{
  "issues": [
    {{
      "line": <line_number>,
      "severity": "<critical|high|medium|low|info>",
      "category": "<bug|security|best-practice|performance|style|maintainability>",
      "message": "<clear description of the issue>",
      "suggestion": "<specific recommendation for fixing>",
      "code_snippet": "<the problematic code>"
    }}
  ],
  "summary": "<overall assessment of code quality>"
}}

If no issues are found, return:
{{
  "issues": [],
  "summary": "No significant issues found. Code quality looks good."
}}
"""


def build_analysis_prompt(language: "LanguageSupport") -> str:
    """Render the shared prompt for one language."""
    if language.conventions:
        bullets = "\n".join(f"- {c}" for c in language.conventions)
        conventions = f"**{language.display_name}-specific concerns:**\n{bullets}\n"
    else:
        conventions = ""
    return _PROMPT_TEMPLATE.format(
        display_name=language.display_name,
        conventions=conventions,
    )
