"""Finding and analysis result models."""

from pydantic import BaseModel, Field


class Typo(BaseModel):
    """Typo with explicit fields for safe fixing."""

    word: str  # The misspelled word
    replacement: str  # The correct spelling
    line: int
    column: int
    context: str  # Surrounding text
    suggestions: list[str] = Field(default_factory=list)  # Alternative corrections


class PatternIssue(BaseModel):
    """Pattern matching issue."""

    type: str  # 'double_space', 'trailing_whitespace', etc.
    line: int
    column: int
    matched_text: str
    replacement: str


class Finding(BaseModel):
    """Generic finding for issue creation."""

    type: str  # 'typo', 'pattern'
    severity: str  # 'info', 'warning', 'error'
    file_path: str
    line: int
    column: int | None = None

    # Explicit fields for safe fixing (Phase 2)
    original: str | None = None
    replacement: str | None = None

    # Human-readable
    message: str
    suggestion: str | None = None


class DocumentationFindings(BaseModel):
    """Results from documentation analysis."""

    file_path: str
    typos: list[Typo] = Field(default_factory=list)
    pattern_issues: list[PatternIssue] = Field(default_factory=list)

    def to_findings(self) -> list[Finding]:
        """Convert to generic Finding objects."""
        findings = [
            Finding(
                type="typo",
                severity="info",
                file_path=self.file_path,
                line=typo.line,
                column=typo.column,
                original=typo.word,
                replacement=typo.replacement,
                message=f"Typo: '{typo.word}'",
                suggestion=f"Did you mean '{typo.replacement}'?",
            )
            for typo in self.typos
        ]

        findings.extend(
            Finding(
                type="pattern",
                severity="info",
                file_path=self.file_path,
                line=issue.line,
                column=issue.column,
                original=issue.matched_text,
                replacement=issue.replacement,
                message=f"Pattern issue: {issue.type}",
                suggestion=f"Replace with: {issue.replacement}",
            )
            for issue in self.pattern_issues
        )

        return findings


class AnalysisResult(BaseModel):
    """What an analyzer pass produced, and what it never got to.

    `failed_files` is the load-bearing half: a caller that only reads
    `findings` cannot distinguish "analyzed and clean" from "the LLM was
    unreachable", which is how a commit gate ends up rubber-stamping.
    """

    findings: list[Finding] = Field(default_factory=list)
    failed_files: list[str] = Field(default_factory=list)
