"""Unit tests for PR review Pydantic schemas."""

import pytest
from pydantic import ValidationError


def test_review_comment_valid():
    """Test ReviewComment validates with correct data."""
    from drep.models.pr_review_findings import ReviewComment

    comment = ReviewComment(
        file_path="src/module.py",
        line=42,
        severity="suggestion",
        comment="Consider adding error handling here",
        suggestion="try:\n    ...\nexcept ValueError:\n    ...",
    )

    assert comment.file_path == "src/module.py"
    assert comment.line == 42
    assert comment.severity == "suggestion"
    assert comment.comment == "Consider adding error handling here"
    assert comment.suggestion is not None


def test_review_comment_without_suggestion():
    """Test ReviewComment works without suggestion field."""
    from drep.models.pr_review_findings import ReviewComment

    comment = ReviewComment(
        file_path="test.py", line=10, severity="info", comment="Good implementation"
    )

    assert comment.suggestion is None


def test_review_comment_severity_validation():
    """Test ReviewComment only accepts valid severity values."""
    from drep.models.pr_review_findings import ReviewComment

    # Valid severities
    for severity in ["info", "suggestion", "warning", "critical"]:
        comment = ReviewComment(file_path="test.py", line=1, severity=severity, comment="Test")
        assert comment.severity == severity

    # Invalid severity should raise ValidationError
    with pytest.raises(ValidationError):
        ReviewComment(file_path="test.py", line=1, severity="invalid", comment="Test")


def test_pr_review_result_valid():
    """Test PRReviewResult validates with correct data."""
    from drep.models.pr_review_findings import PRReviewResult, ReviewComment

    result = PRReviewResult(
        comments=[
            ReviewComment(
                file_path="src/file.py",
                line=10,
                severity="warning",
                comment="Missing type hint",
            )
        ],
        summary="Overall the PR looks good with minor issues",
        approve=True,
        concerns=["Missing tests"],
    )

    assert len(result.comments) == 1
    assert result.summary == "Overall the PR looks good with minor issues"
    assert result.approve is True
    assert len(result.concerns) == 1


def test_pr_review_result_empty_comments():
    """Test PRReviewResult works with empty comments list."""
    from drep.models.pr_review_findings import PRReviewResult

    result = PRReviewResult(comments=[], summary="No issues found", approve=True, concerns=[])

    assert result.comments == []
    assert result.approve is True


def test_pr_review_result_default_values():
    """Test PRReviewResult defaults for optional fields."""
    from drep.models.pr_review_findings import PRReviewResult

    result = PRReviewResult(summary="Test summary", approve=False)

    # comments and concerns should default to empty lists
    assert result.comments == []
    assert result.concerns == []


def test_review_comment_required_fields():
    """Test ReviewComment requires file_path, line, severity, comment."""
    from drep.models.pr_review_findings import ReviewComment

    # Missing required fields should raise ValidationError
    with pytest.raises(ValidationError):
        ReviewComment(line=1, severity="info", comment="Test")  # Missing file_path

    with pytest.raises(ValidationError):
        ReviewComment(file_path="test.py", severity="info", comment="Test")  # Missing line

    with pytest.raises(ValidationError):
        ReviewComment(file_path="test.py", line=1, comment="Test")  # Missing severity

    with pytest.raises(ValidationError):
        ReviewComment(file_path="test.py", line=1, severity="info")  # Missing comment


def test_pr_review_result_required_fields():
    """Test PRReviewResult requires summary and approve fields."""
    from drep.models.pr_review_findings import PRReviewResult

    # Missing required fields should raise ValidationError
    with pytest.raises(ValidationError):
        PRReviewResult(approve=True)  # Missing summary

    with pytest.raises(ValidationError):
        PRReviewResult(summary="Test")  # Missing approve


def test_review_comment_line_positive():
    """Test ReviewComment line number must be positive."""
    from drep.models.pr_review_findings import ReviewComment

    # Positive line numbers should work
    comment = ReviewComment(file_path="test.py", line=1, severity="info", comment="Test")
    assert comment.line == 1

    # Zero or negative should raise ValidationError
    with pytest.raises(ValidationError):
        ReviewComment(file_path="test.py", line=0, severity="info", comment="Test")

    with pytest.raises(ValidationError):
        ReviewComment(file_path="test.py", line=-1, severity="info", comment="Test")


def test_pr_review_result_json_serialization():
    """Test PRReviewResult can be serialized to JSON."""
    from drep.models.pr_review_findings import PRReviewResult, ReviewComment

    result = PRReviewResult(
        comments=[
            ReviewComment(
                file_path="test.py",
                line=5,
                severity="suggestion",
                comment="Use type hints",
                suggestion="def func(x: int) -> str:",
            )
        ],
        summary="Good PR with suggestions",
        approve=True,
        concerns=[],
    )

    # Should serialize to dict
    data = result.model_dump()
    assert isinstance(data, dict)
    assert data["summary"] == "Good PR with suggestions"
    assert data["approve"] is True
    assert len(data["comments"]) == 1
    assert data["comments"][0]["file_path"] == "test.py"
