"""Tests for docstring generation Pydantic schemas."""

import pytest
from pydantic import ValidationError

from drep.models.docstring_findings import (
    DocstringGenerationResult,
    DocstringQualityResult,
)


class TestDocstringGenerationResult:
    """Tests for DocstringGenerationResult schema."""

    def test_valid_docstring_generation_result(self):
        """Test creating valid docstring generation result."""
        docstring_text = (
            "Calculate the sum of two numbers.\n\n"
            "Args:\n    a: First number\n    b: Second number\n\n"
            "Returns:\n    Sum of a and b"
        )
        result = DocstringGenerationResult(
            docstring=docstring_text,
            quality="high",
            reasoning="Function performs simple addition with clear inputs and output",
        )

        assert result.docstring.startswith("Calculate the sum")
        assert result.quality == "high"
        assert "addition" in result.reasoning

    def test_quality_must_be_valid_literal(self):
        """Test that quality must be one of: high, medium, low."""
        # Valid values
        for quality in ["high", "medium", "low"]:
            result = DocstringGenerationResult(
                docstring="Test docstring",
                quality=quality,
                reasoning="Test reasoning",
            )
            assert result.quality == quality

        # Invalid value should raise validation error
        with pytest.raises(ValidationError):
            DocstringGenerationResult(
                docstring="Test docstring",
                quality="invalid",  # Not a valid literal
                reasoning="Test reasoning",
            )

    def test_all_fields_required(self):
        """Test that all fields are required."""
        # Missing docstring
        with pytest.raises(ValidationError):
            DocstringGenerationResult(
                quality="high",
                reasoning="Test reasoning",
            )

        # Missing quality
        with pytest.raises(ValidationError):
            DocstringGenerationResult(
                docstring="Test docstring",
                reasoning="Test reasoning",
            )

        # Missing reasoning
        with pytest.raises(ValidationError):
            DocstringGenerationResult(
                docstring="Test docstring",
                quality="high",
            )

    def test_docstring_can_be_multiline(self):
        """Test that docstring field can contain newlines."""
        multiline_docstring = """Brief description.

Longer explanation with multiple paragraphs.

Args:
    param1: Description of param1.
    param2: Description of param2.

Returns:
    Description of return value.

Raises:
    ValueError: When validation fails.
"""
        result = DocstringGenerationResult(
            docstring=multiline_docstring,
            quality="high",
            reasoning="Complete docstring with all sections",
        )

        assert "\n" in result.docstring
        assert "Args:" in result.docstring
        assert "Returns:" in result.docstring
        assert "Raises:" in result.docstring


class TestDocstringQualityResult:
    """Tests for DocstringQualityResult schema."""

    def test_valid_quality_result(self):
        """Test creating valid quality result."""
        result = DocstringQualityResult(
            quality="low",
            issues=["Missing arg descriptions", "Too generic"],
            needs_rewrite=True,
            reasoning="Docstring lacks detail and specificity",
        )

        assert result.quality == "low"
        assert len(result.issues) == 2
        assert result.needs_rewrite is True
        assert "lacks detail" in result.reasoning

    def test_issues_can_be_empty(self):
        """Test that issues list can be empty for high-quality docstrings."""
        result = DocstringQualityResult(
            quality="high",
            issues=[],
            needs_rewrite=False,
            reasoning="Docstring is well-written and complete",
        )

        assert result.quality == "high"
        assert result.issues == []
        assert result.needs_rewrite is False

    def test_quality_literal_validation(self):
        """Test that quality must be valid literal."""
        # Valid values
        for quality in ["high", "medium", "low"]:
            result = DocstringQualityResult(
                quality=quality,
                issues=[],
                needs_rewrite=False,
                reasoning="Test",
            )
            assert result.quality == quality

        # Invalid value
        with pytest.raises(ValidationError):
            DocstringQualityResult(
                quality="terrible",  # Invalid
                issues=[],
                needs_rewrite=True,
                reasoning="Test",
            )

    def test_needs_rewrite_is_required(self):
        """Test that needs_rewrite field is required."""
        with pytest.raises(ValidationError):
            DocstringQualityResult(
                quality="medium",
                issues=["Some issue"],
                reasoning="Test",
            )

    def test_issues_list_with_multiple_items(self):
        """Test issues list with multiple quality issues."""
        issues_list = [
            "Missing return type documentation",
            "No examples provided",
            "Argument descriptions are too brief",
            "Does not follow Google style guide",
        ]

        result = DocstringQualityResult(
            quality="medium",
            issues=issues_list,
            needs_rewrite=False,
            reasoning="Some issues but not critical",
        )

        assert len(result.issues) == 4
        assert "Missing return type documentation" in result.issues
