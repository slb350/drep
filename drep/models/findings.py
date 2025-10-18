"""Finding and analysis result models."""

from typing import List, Optional

from pydantic import BaseModel, Field


class Typo(BaseModel):
    """Typo with explicit fields for safe fixing."""

    word: str  # The misspelled word
    replacement: str  # The correct spelling
    line: int
    column: int
    context: str  # Surrounding text
    suggestions: List[str] = Field(default_factory=list)  # Alternative corrections


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
    column: Optional[int] = None

    # Explicit fields for safe fixing (Phase 2)
    original: Optional[str] = None
    replacement: Optional[str] = None

    # Human-readable
    message: str
    suggestion: Optional[str] = None


class DocumentationFindings(BaseModel):
    """Results from documentation analysis."""

    file_path: str
    typos: List[Typo] = Field(default_factory=list)
    pattern_issues: List[PatternIssue] = Field(default_factory=list)

    def to_findings(self) -> List[Finding]:
        """Convert to generic Finding objects."""
        findings = []

        for typo in self.typos:
            findings.append(
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
            )

        for issue in self.pattern_issues:
            findings.append(
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
            )

        return findings
