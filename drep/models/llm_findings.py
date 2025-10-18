"""LLM-generated findings and code analysis results."""

from typing import List, Literal

from pydantic import BaseModel, Field

from drep.models.findings import Finding


class CodeIssue(BaseModel):
    """Single code quality issue found by LLM.

    This schema is used to structure LLM responses for code analysis.
    The LLM is instructed to return JSON matching this schema.
    """

    line: int = Field(..., description="Line number where the issue occurs", ge=1)
    severity: Literal["critical", "high", "medium", "low", "info"] = Field(
        ..., description="Severity level of the issue"
    )
    category: str = Field(
        ...,
        description="Issue category (e.g., 'bug', 'security', 'best-practice', 'performance', 'style')",
    )
    message: str = Field(..., description="Clear description of the issue")
    suggestion: str = Field(..., description="Specific recommendation for fixing the issue")
    code_snippet: str = Field(..., description="The problematic code fragment")


class CodeAnalysisResult(BaseModel):
    """Result of LLM code analysis.

    This is the top-level schema returned by the LLM when analyzing code.
    Contains a list of issues and an overall summary.
    """

    issues: List[CodeIssue] = Field(
        default_factory=list, description="List of code quality issues found"
    )
    summary: str = Field(..., description="Overall code quality summary and recommendations")

    def to_findings(self, file_path: str) -> List[Finding]:
        """Convert CodeAnalysisResult to generic Finding objects.

        Args:
            file_path: Path to the analyzed file

        Returns:
            List of Finding objects compatible with IssueManager
        """
        findings = []

        for issue in self.issues:
            # Map LLM severity to Finding severity
            # LLM uses: critical, high, medium, low, info
            # Finding uses: error, warning, info
            if issue.severity in ("critical", "high"):
                finding_severity = "error"
            elif issue.severity == "medium":
                finding_severity = "warning"
            else:
                finding_severity = "info"

            finding = Finding(
                type=issue.category,
                severity=finding_severity,
                file_path=file_path,
                line=issue.line,
                column=None,  # LLM doesn't provide column info
                original=issue.code_snippet,
                replacement=None,  # LLM provides suggestions, not direct replacements
                message=issue.message,
                suggestion=issue.suggestion,
            )
            findings.append(finding)

        return findings
