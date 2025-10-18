"""Pydantic schemas for docstring generation results."""

from typing import List, Literal

from pydantic import BaseModel, Field


class DocstringGenerationResult(BaseModel):
    """Result of LLM docstring generation."""

    docstring: str = Field(..., description="Generated Google-style docstring")
    quality: Literal["high", "medium", "low"] = Field(
        ..., description="Confidence in docstring quality"
    )
    reasoning: str = Field(..., description="Explanation of what function does")


class DocstringQualityResult(BaseModel):
    """Result of docstring quality assessment."""

    quality: Literal["high", "medium", "low"] = Field(..., description="Docstring quality rating")
    issues: List[str] = Field(default_factory=list, description="List of quality issues found")
    needs_rewrite: bool = Field(..., description="Whether docstring should be rewritten")
    reasoning: str = Field(..., description="Explanation of quality assessment")
