"""Finding and analysis result models."""

from enum import Enum

from pydantic import BaseModel, Field


class Severity(str, Enum):
    """Finding severity, lowest first.

    The single vocabulary for `Finding.severity`. Producers map their own
    scales onto it (see llm_findings.to_findings); consumers that gate on
    severity order it with SEVERITY_RANK rather than inventing a ranking.
    """

    # (str, Enum) rather than StrEnum: this package supports Python 3.10.
    INFO = "info"
    WARNING = "warning"
    ERROR = "error"

    def __str__(self) -> str:
        """Render as the bare value.

        Without this, f-strings interpolate `Severity.WARNING` - which lands in
        issue bodies and CLI output. StrEnum gets this for free; (str, Enum) on
        3.11+ does not.
        """
        return self.value


# Ordered ranks for threshold comparisons ("block at or above this severity").
SEVERITY_RANK: dict[str, int] = {s.value: rank for rank, s in enumerate(Severity)}


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
    severity: Severity
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
    """What an analysis produced, and what it never got to.

    Two fields carry the weight:

    - `failed_files`: a caller that only reads `findings` cannot distinguish
      "analyzed and clean" from "the LLM was unreachable", which is how a
      commit gate ends up rubber-stamping.
    - `blocking`: findings from the project's own deterministic tools. They are
      kept apart from `findings` by *source*, not severity, because that is
      what makes a gate calibratable - ruff and eslint are precise enough to
      block, an LLM is not.
    """

    findings: list[Finding] = Field(default_factory=list)
    failed_files: list[str] = Field(default_factory=list)
    blocking: list[Finding] = Field(default_factory=list)
    # Tools that should have run and could not. Kept apart from failed_files
    # because "eslint is missing" and "this file went unanalyzed" need
    # different words, even though both mean the result is incomplete.
    unavailable_tools: list[str] = Field(default_factory=list)

    @property
    def incomplete(self) -> bool:
        """Whether anything that should have been checked was not."""
        return bool(self.failed_files or self.unavailable_tools)
