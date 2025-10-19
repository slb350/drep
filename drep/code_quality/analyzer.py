"""LLM-powered code quality analyzer."""

import logging
from pathlib import Path
from typing import List

from drep.llm.client import LLMClient
from drep.models.findings import Finding
from drep.models.llm_findings import CodeAnalysisResult

logger = logging.getLogger(__name__)

# Maximum file size to analyze (in characters)
# Approximately 8k tokens (assuming ~4 chars per token)
MAX_FILE_SIZE = 32000

PYTHON_ANALYSIS_PROMPT = """You are an expert Python code reviewer. Analyze the following code and identify issues in these categories:

1. **Bugs & Logic Errors**: Incorrect logic, unhandled edge cases, potential crashes, undefined variables, type errors
2. **Security Issues**: SQL injection, command injection, path traversal, unsafe deserialization, hardcoded secrets, weak cryptography
3. **Best Practices**: PEP 8 violations, missing docstrings, poor naming conventions, code smells, anti-patterns
4. **Performance**: Inefficient algorithms, unnecessary loops, blocking I/O operations, memory leaks

For each issue found, provide:
- Line number (approximate if exact line is unclear)
- Severity: critical (security vulnerabilities, crashes), high (bugs, serious issues), medium (best practices, moderate issues), low (minor improvements), info (suggestions)
- Category: bug, security, best-practice, performance, style, maintainability
- Clear message explaining the issue
- Specific, actionable suggestion for fixing it
- The problematic code snippet

**Important instructions:**
- Only report genuine issues, not false positives
- Be specific about line numbers - estimate if needed
- Provide actionable suggestions, not vague advice
- Focus on correctness, security, and maintainability
- Do not report style issues that are subjective

Return your analysis as valid JSON matching this exact schema:
{
  "issues": [
    {
      "line": <line_number>,
      "severity": "<critical|high|medium|low|info>",
      "category": "<bug|security|best-practice|performance|style|maintainability>",
      "message": "<clear description of the issue>",
      "suggestion": "<specific recommendation for fixing>",
      "code_snippet": "<the problematic code>"
    }
  ],
  "summary": "<overall assessment of code quality>"
}

If no issues are found, return:
{
  "issues": [],
  "summary": "No significant issues found. Code quality looks good."
}
"""


class CodeQualityAnalyzer:
    """LLM-powered code quality analyzer for Python files.

    Uses an LLM client to perform intelligent code analysis, detecting bugs,
    security issues, best practice violations, and performance problems.
    """

    def __init__(self, llm_client: LLMClient):
        """Initialize analyzer with LLM client.

        Args:
            llm_client: Configured LLMClient instance for making analysis requests
        """
        self.llm_client = llm_client

    async def analyze_file(
        self, file_path: str, content: str, repo_id: str, commit_sha: str
    ) -> List[Finding]:
        """Analyze Python file for code quality issues.

        Args:
            file_path: Path to the file being analyzed
            content: File content to analyze
            repo_id: Repository identifier for rate limiting
            commit_sha: Current commit SHA for cache invalidation

        Returns:
            List of Finding objects describing issues found

        Note:
            - Files larger than MAX_FILE_SIZE (32k chars) are skipped
            - LLM failures are logged but don't raise exceptions
            - Returns empty list if analysis fails or file is too large
        """
        # Check file size limit
        if len(content) > MAX_FILE_SIZE:
            logger.warning(
                f"Skipping {file_path}: file too large ({len(content)} chars, max {MAX_FILE_SIZE})"
            )
            return []

        # Skip empty files
        if not content.strip():
            logger.debug(f"Skipping {file_path}: empty file")
            return []

        try:
            # Call LLM with structured schema
            logger.debug(f"Analyzing {file_path} ({len(content)} chars)")

            result_dict = await self.llm_client.analyze_code_json(
                system_prompt=PYTHON_ANALYSIS_PROMPT,
                code=content,
                schema=CodeAnalysisResult,
                repo_id=repo_id,
                commit_sha=commit_sha,
                analyzer="code_quality",
            )

            # Convert dict to Pydantic model
            result = CodeAnalysisResult(**result_dict)

            # Log analysis results
            logger.info(
                f"Analyzed {file_path}: found {len(result.issues)} issues "
                f"({sum(1 for i in result.issues if i.severity in ['critical', 'high'])} critical/high)"
            )

            # Convert to Finding objects
            findings = result.to_findings(file_path)
            return findings

        except ValueError as e:
            # JSON parsing failed after all strategies
            logger.error(f"Failed to parse LLM response for {file_path}: {e}")
            return []

        except Exception as e:
            # LLM request failed or other error
            logger.error(f"LLM analysis failed for {file_path}: {e}", exc_info=True)
            return []

    async def analyze_files(
        self, files: List[tuple[str, str]], repo_id: str, commit_sha: str
    ) -> List[Finding]:
        """Analyze multiple Python files.

        Args:
            files: List of (file_path, content) tuples
            repo_id: Repository identifier for rate limiting
            commit_sha: Current commit SHA for cache invalidation

        Returns:
            Combined list of findings from all files
        """
        all_findings: List[Finding] = []

        for file_path, content in files:
            findings = await self.analyze_file(file_path, content, repo_id, commit_sha)
            all_findings.extend(findings)

        return all_findings

    def is_supported_file(self, file_path: str) -> bool:
        """Check if file is supported for analysis.

        Currently only Python files (.py) are supported.

        Args:
            file_path: Path to check

        Returns:
            True if file should be analyzed, False otherwise
        """
        path = Path(file_path)
        return path.suffix == ".py"
