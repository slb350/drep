"""Unit tests for PR Review Analyzer."""

from unittest.mock import AsyncMock

import pytest


@pytest.mark.asyncio
async def test_review_pr_success():
    """Test review_pr() successfully reviews a PR end-to-end."""
    from drep.models.pr_review_findings import PRReviewResult
    from drep.pr_review.analyzer import PRReviewAnalyzer

    # Mock Gitea adapter
    gitea = AsyncMock()
    gitea.get_pr.return_value = {
        "number": 42,
        "title": "Add feature X",
        "body": "Description",
        "head": {"sha": "abc123"},
        "user": {"login": "steve"},
        "base": {"ref": "main"},
    }
    gitea.get_pr_diff.return_value = """diff --git a/test.py b/test.py
@@ -1,2 +1,3 @@
 def test():
+    print("new line")
     pass"""

    # Mock LLM client
    llm = AsyncMock()
    llm.analyze_code_json.return_value = {
        "comments": [
            {
                "file_path": "test.py",
                "line": 2,
                "severity": "suggestion",
                "comment": "Good addition",
                "suggestion": None,
            }
        ],
        "summary": "Looks good",
        "approve": True,
        "concerns": [],
    }

    analyzer = PRReviewAnalyzer(llm, gitea)
    result = await analyzer.review_pr("steve", "drep", 42)

    assert isinstance(result, PRReviewResult)
    assert result.summary == "Looks good"
    assert result.approve is True
    assert len(result.comments) == 1
    assert result.comments[0].file_path == "test.py"

    # Verify Gitea methods were called
    gitea.get_pr.assert_called_once_with("steve", "drep", 42)
    gitea.get_pr_diff.assert_called_once_with("steve", "drep", 42)

    # Verify LLM was called
    llm.analyze_code_json.assert_called_once()


@pytest.mark.asyncio
async def test_review_pr_truncates_large_diff():
    """Test that large diffs (> 20k chars) are truncated."""
    from drep.pr_review.analyzer import PRReviewAnalyzer

    gitea = AsyncMock()
    gitea.get_pr.return_value = {
        "number": 42,
        "title": "Big PR",
        "body": "Large changes",
        "head": {"sha": "abc123"},
        "user": {"login": "steve"},
        "base": {"ref": "main"},
    }

    # Create a properly formatted diff > 20k chars
    large_diff = (
        """diff --git a/file.py b/file.py
@@ -1,1 +1,1000 @@
 old line
"""
        + ("+" + "x" * 1000 + "\n") * 30
    )
    assert len(large_diff) > 20000

    gitea.get_pr_diff.return_value = large_diff

    llm = AsyncMock()
    llm.analyze_code_json.return_value = {
        "comments": [],
        "summary": "Too large to review fully",
        "approve": False,
        "concerns": ["Diff truncated due to size"],
    }

    analyzer = PRReviewAnalyzer(llm, gitea)
    await analyzer.review_pr("steve", "drep", 42)

    # Verify LLM was called with truncated diff
    call_args = llm.analyze_code_json.call_args
    prompt = call_args[1]["system_prompt"]

    # The prompt should mention truncation
    assert "TRUNCATED" in prompt or "truncated" in prompt.lower()


@pytest.mark.asyncio
async def test_analyze_diff_with_llm():
    """Test _analyze_diff_with_llm() constructs correct prompt."""
    from drep.pr_review.analyzer import PRReviewAnalyzer
    from drep.pr_review.diff_parser import DiffHunk

    gitea = AsyncMock()
    llm = AsyncMock()
    llm.analyze_code_json.return_value = {
        "comments": [],
        "summary": "Test",
        "approve": True,
        "concerns": [],
    }

    analyzer = PRReviewAnalyzer(llm, gitea)

    pr_data = {
        "number": 42,
        "title": "Test PR",
        "body": "Test description",
        "user": {"login": "steve"},
        "base": {"ref": "main"},
        "head": {"ref": "feature"},
    }

    hunks = [
        DiffHunk(
            file_path="test.py",
            old_start=1,
            old_count=2,
            new_start=1,
            new_count=3,
            lines=[" line1", "+line2", " line3"],
        )
    ]

    await analyzer._analyze_diff_with_llm(pr_data, hunks, "steve/drep")

    # Verify LLM was called
    assert llm.analyze_code_json.called

    # Check that prompt includes PR details
    call_args = llm.analyze_code_json.call_args
    prompt = call_args[1]["system_prompt"]

    assert "Test PR" in prompt
    assert "steve" in prompt
    assert "test.py" in prompt


@pytest.mark.asyncio
async def test_post_review_creates_comments():
    """Test post_review() posts summary and inline comments."""
    from drep.models.pr_review_findings import PRReviewResult, ReviewComment
    from drep.pr_review.analyzer import PRReviewAnalyzer
    from drep.pr_review.diff_parser import DiffHunk

    gitea = AsyncMock()
    llm = AsyncMock()

    analyzer = PRReviewAnalyzer(llm, gitea)

    # Set up hunks so the comments are valid
    # Lines 1-9 are context, line 10 is added, lines 11-19 are context, line 20 is added
    analyzer._current_hunks = [
        DiffHunk(
            file_path="src/file.py",
            old_start=1,
            old_count=18,
            new_start=1,
            new_count=20,
            lines=[
                " line1",
                " line2",
                " line3",
                " line4",
                " line5",
                " line6",
                " line7",
                " line8",
                " line9",
                "+line 10 added",
                " line11",
                " line12",
                " line13",
                " line14",
                " line15",
                " line16",
                " line17",
                " line18",
                " line19",
                "+line 20 added",
            ],
        )
    ]

    result = PRReviewResult(
        comments=[
            ReviewComment(
                file_path="src/file.py",
                line=10,
                severity="warning",
                comment="Fix this",
                suggestion="x = 1",
            ),
            ReviewComment(
                file_path="src/file.py",
                line=20,
                severity="info",
                comment="Good job",
            ),
        ],
        summary="Overall good PR",
        approve=True,
        concerns=[],
    )

    await analyzer.post_review("steve", "drep", 42, "abc123", result)

    # Should create 1 summary comment
    gitea.create_pr_comment.assert_called_once()
    summary_call = gitea.create_pr_comment.call_args
    body = summary_call.kwargs["body"]
    assert "Overall good PR" in body
    assert "Approve" in body or "✅" in body

    # Should create 2 inline comments
    assert gitea.create_pr_review_comment.call_count == 2

    # Verify inline comment calls
    calls = gitea.create_pr_review_comment.call_args_list
    assert calls[0].kwargs["line"] == 10
    assert "Fix this" in calls[0].kwargs["body"]
    assert calls[1].kwargs["line"] == 20
    assert "Good job" in calls[1].kwargs["body"]


@pytest.mark.asyncio
async def test_post_review_no_approval():
    """Test post_review() shows 'Needs Changes' when not approved."""
    from drep.models.pr_review_findings import PRReviewResult
    from drep.pr_review.analyzer import PRReviewAnalyzer

    gitea = AsyncMock()
    llm = AsyncMock()

    analyzer = PRReviewAnalyzer(llm, gitea)

    result = PRReviewResult(
        comments=[],
        summary="Issues found",
        approve=False,
        concerns=["Missing tests"],
    )

    await analyzer.post_review("steve", "drep", 42, "abc123", result)

    # Summary should indicate changes needed
    summary_call = gitea.create_pr_comment.call_args
    body = summary_call.kwargs["body"]
    assert "Needs Changes" in body or "🔍" in body or "concerns" in body.lower()


@pytest.mark.asyncio
async def test_review_pr_handles_gitea_error():
    """Test review_pr() handles Gitea errors gracefully."""
    from drep.pr_review.analyzer import PRReviewAnalyzer

    gitea = AsyncMock()
    gitea.get_pr.side_effect = ValueError("PR not found")

    llm = AsyncMock()

    analyzer = PRReviewAnalyzer(llm, gitea)

    # Should raise the Gitea error
    with pytest.raises(ValueError, match="PR not found"):
        await analyzer.review_pr("steve", "drep", 999)


@pytest.mark.asyncio
async def test_review_pr_handles_llm_error():
    """Test review_pr() handles LLM errors gracefully."""
    from drep.pr_review.analyzer import PRReviewAnalyzer

    gitea = AsyncMock()
    gitea.get_pr.return_value = {
        "number": 42,
        "title": "Test",
        "body": "Test",
        "head": {"sha": "abc"},
        "user": {"login": "steve"},
        "base": {"ref": "main"},
    }
    gitea.get_pr_diff.return_value = "diff --git a/test.py b/test.py\n"

    llm = AsyncMock()
    llm.analyze_code_json.side_effect = ValueError("LLM connection failed")

    analyzer = PRReviewAnalyzer(llm, gitea)

    # Should propagate the LLM error
    with pytest.raises(ValueError, match="LLM connection failed"):
        await analyzer.review_pr("steve", "drep", 42)


@pytest.mark.asyncio
async def test_post_review_skips_if_no_comments():
    """Test post_review() only posts summary if no inline comments."""
    from drep.models.pr_review_findings import PRReviewResult
    from drep.pr_review.analyzer import PRReviewAnalyzer

    gitea = AsyncMock()
    llm = AsyncMock()

    analyzer = PRReviewAnalyzer(llm, gitea)

    result = PRReviewResult(
        comments=[],  # No inline comments
        summary="No issues found",
        approve=True,
        concerns=[],
    )

    await analyzer.post_review("steve", "drep", 42, "abc123", result)

    # Should create summary comment
    gitea.create_pr_comment.assert_called_once()

    # Should NOT create inline comments
    gitea.create_pr_review_comment.assert_not_called()


@pytest.mark.asyncio
async def test_truncate_diff_strategy():
    """Test diff truncation keeps first 15k and last 5k chars."""
    from drep.pr_review.analyzer import PRReviewAnalyzer

    gitea = AsyncMock()
    llm = AsyncMock()
    llm.analyze_code_json.return_value = {
        "comments": [],
        "summary": "Test",
        "approve": True,
        "concerns": [],
    }

    # Create large diff with proper hunk lines (> 20k chars total when reconstructed)
    hunk_lines = "+line_" + "A" * 100 + "\n"  # ~106 chars per line
    large_diff = """diff --git a/test.py b/test.py
@@ -1,1 +1,300 @@
 header
""" + (
        hunk_lines * 200
    )  # 200 lines * 106 chars = ~21k chars

    gitea.get_pr.return_value = {
        "number": 1,
        "title": "T",
        "body": "B",
        "head": {"sha": "x"},
        "user": {"login": "u"},
        "base": {"ref": "m"},
    }
    gitea.get_pr_diff.return_value = large_diff

    analyzer = PRReviewAnalyzer(llm, gitea)
    await analyzer.review_pr("o", "r", 1)

    # Extract the diff content sent to LLM
    call_args = llm.analyze_code_json.call_args
    prompt = call_args[1]["system_prompt"]

    # Should contain TRUNCATED marker
    assert "TRUNCATED" in prompt or "truncated" in prompt.lower()


@pytest.mark.asyncio
async def test_post_review_skips_invalid_line_numbers():
    """Test post_review() skips comments with invalid line numbers."""
    from drep.models.pr_review_findings import PRReviewResult, ReviewComment
    from drep.pr_review.analyzer import PRReviewAnalyzer
    from drep.pr_review.diff_parser import DiffHunk

    gitea = AsyncMock()
    llm = AsyncMock()

    analyzer = PRReviewAnalyzer(llm, gitea)

    # Set up hunks to simulate a diff with specific added lines
    analyzer._current_hunks = [
        DiffHunk(
            file_path="src/file.py",
            old_start=1,
            old_count=2,
            new_start=1,
            new_count=3,
            lines=[" old line", "+new line at 2", " old line"],
        )
    ]

    # Create result with one valid and one invalid line number
    result = PRReviewResult(
        comments=[
            ReviewComment(
                file_path="src/file.py",
                line=2,  # Valid - exists in diff
                severity="warning",
                comment="Valid comment",
            ),
            ReviewComment(
                file_path="src/file.py",
                line=10,  # Invalid - not in diff
                severity="warning",
                comment="Invalid line number",
            ),
            ReviewComment(
                file_path="other.py",
                line=5,  # Invalid - file not in diff
                severity="info",
                comment="File not in diff",
            ),
        ],
        summary="Mixed valid/invalid comments",
        approve=True,
        concerns=[],
    )

    await analyzer.post_review("steve", "drep", 42, "abc123", result)

    # Should create summary comment
    gitea.create_pr_comment.assert_called_once()

    # Should only create 1 inline comment (the valid one)
    assert gitea.create_pr_review_comment.call_count == 1

    # Verify it was the valid comment that was posted
    call = gitea.create_pr_review_comment.call_args
    assert call.kwargs["line"] == 2
    assert "Valid comment" in call.kwargs["body"]


@pytest.mark.asyncio
async def test_is_valid_comment_line():
    """Test _is_valid_comment_line() validates lines correctly."""
    from drep.pr_review.analyzer import PRReviewAnalyzer
    from drep.pr_review.diff_parser import DiffHunk

    gitea = AsyncMock()
    llm = AsyncMock()

    analyzer = PRReviewAnalyzer(llm, gitea)

    # Set up hunks with specific added lines
    # test.py: line 1 context, line 2 added, line 3 context, line 4 added, line 5 removed, line 5 context, line 6 added
    # After the removed line, the new line number continues: 1(ctx), 2(add), 3(ctx), 4(add), -(removed), 5(ctx), 6(add)
    analyzer._current_hunks = [
        DiffHunk(
            file_path="test.py",
            old_start=1,
            old_count=6,
            new_start=1,
            new_count=6,
            lines=[
                " line 1",
                "+line 2 (added)",
                " line 3",
                "+line 4 (added)",
                "-line removed",
                " line 5",
                "+line 6 (added)",
            ],
        ),
        DiffHunk(
            file_path="other.py",
            old_start=10,
            old_count=2,
            new_start=10,
            new_count=3,
            lines=[" line 10", "+line 11 (added)", " line 12"],
        ),
    ]

    # Test valid lines (added lines)
    assert analyzer._is_valid_comment_line("test.py", 2) is True
    assert analyzer._is_valid_comment_line("test.py", 4) is True
    assert analyzer._is_valid_comment_line("test.py", 6) is True  # Fixed: line 6, not 7
    assert analyzer._is_valid_comment_line("other.py", 11) is True

    # Test invalid lines (not added, removed, or non-existent)
    assert analyzer._is_valid_comment_line("test.py", 1) is False  # Context line
    assert analyzer._is_valid_comment_line("test.py", 3) is False  # Context line
    assert analyzer._is_valid_comment_line("test.py", 5) is False  # Context line
    assert analyzer._is_valid_comment_line("test.py", 999) is False  # Doesn't exist
    assert analyzer._is_valid_comment_line("nonexistent.py", 1) is False  # File not in diff
