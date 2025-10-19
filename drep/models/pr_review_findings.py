"""Pydantic schemas for PR review findings."""

from typing import List, Literal, Optional

from pydantic import BaseModel, Field


class ReviewComment(BaseModel):
    """Single inline review comment for a PR.

    Represents a comment on a specific line in a specific file.
    Used by the LLM to provide feedback on changed code.
    """

    file_path: str = Field(..., description="File being reviewed (relative to repo root)")
    line: int = Field(..., gt=0, description="Line number in new version (must be > 0)")
    severity: Literal["info", "suggestion", "warning", "critical"] = Field(
        ..., description="Severity level of the comment"
    )
    comment: str = Field(..., description="Review comment text")
    suggestion: Optional[str] = Field(None, description="Suggested code fix (optional)")


class PRReviewResult(BaseModel):
    """Complete PR review result from LLM analysis.

    Contains all review comments, overall summary, approval status,
    and any major concerns.
    """

    comments: List[ReviewComment] = Field(
        default_factory=list, description="List of inline review comments"
    )
    summary: str = Field(..., description="Overall PR assessment")
    approve: bool = Field(..., description="Whether to approve the PR")
    concerns: List[str] = Field(default_factory=list, description="Major issues or blockers")
