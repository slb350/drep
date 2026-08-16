"""Unit tests for PR Review Analyzer."""

from unittest.mock import AsyncMock, Mock

import pytest

from drep.adapters.base import ReviewAnchor


def _anchor(owner: str = "steve", repo: str = "drep", pr_number: int = 42, sha: str = "abc123"):
    return ReviewAnchor(owner=owner, repo=repo, pr_number=pr_number, commit_sha=sha)


def _mock_adapter(pr_data: dict | None = None, diff_text: str = "") -> AsyncMock:
    """Build a mocked adapter deriving its anchor from the PR payload."""
    gitea = AsyncMock()
    # anchor_from_pr is a pure (sync) derivation, not a coroutine
    gitea.anchor_from_pr = Mock(return_value=_anchor())
    gitea.get_pr.return_value = pr_data or {
        "number": 42,
        "title": "Add feature X",
        "body": "Description",
        "head": {"sha": "abc123"},
        "user": {"login": "steve"},
        "base": {"ref": "main"},
    }
    gitea.get_pr_diff.return_value = diff_text
    return gitea


@pytest.mark.asyncio
async def test_review_pr_success():
    """Test review_pr() returns a PreparedReview bound to anchor and added lines."""
    from drep.models.pr_review_findings import PRReviewResult
    from drep.pr_review.analyzer import PreparedReview, PRReviewAnalyzer

    gitea = _mock_adapter(
        diff_text="""diff --git a/test.py b/test.py
@@ -1,2 +1,3 @@
 def test():
+    print("new line")
     pass"""
    )

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
    prepared = await analyzer.review_pr("steve", "drep", 42)

    assert isinstance(prepared, PreparedReview)
    assert isinstance(prepared.result, PRReviewResult)
    assert prepared.result.summary == "Looks good"
    assert prepared.result.approve is True
    assert len(prepared.result.comments) == 1
    assert prepared.result.comments[0].file_path == "test.py"

    # Anchor and added-lines index are bound to the prepared review
    assert prepared.anchor == _anchor()
    assert prepared.added_lines == {"test.py": frozenset({2})}

    # The PR payload is fetched exactly once and reused for prompt and anchor
    gitea.get_pr.assert_called_once_with("steve", "drep", 42)
    gitea.anchor_from_pr.assert_called_once()
    gitea.get_pr_diff.assert_called_once_with("steve", "drep", 42)

    # Verify LLM was called
    llm.analyze_code_json.assert_called_once()


@pytest.mark.asyncio
async def test_review_pr_truncates_large_diff():
    """Test that large diffs (> 20k chars) are truncated."""
    from drep.pr_review.analyzer import PRReviewAnalyzer

    # Create a properly formatted diff > 20k chars
    large_diff = (
        """diff --git a/file.py b/file.py
@@ -1,1 +1,1000 @@
 old line
"""
        + ("+" + "x" * 1000 + "\n") * 30
    )
    assert len(large_diff) > 20000

    gitea = _mock_adapter(diff_text=large_diff)

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
    diff_text = "diff --git a/test.py b/test.py\n@@ -1,2 +1,3 @@\n line1\n+line2\n line3\n"

    await analyzer._analyze_diff_with_llm(pr_data, diff_text, hunks, "steve/drep", "abc123")

    # Verify LLM was called
    assert llm.analyze_code_json.called

    call_args = llm.analyze_code_json.call_args
    prompt = call_args[1]["system_prompt"]

    # Check that prompt includes PR details
    assert "Test PR" in prompt
    assert "steve" in prompt
    assert "test.py" in prompt
    # The fetched diff is passed through verbatim, not re-serialized from hunks
    assert diff_text in prompt
    # repo_id and commit_sha reach the LLM client (per-repo limiting, cache key)
    assert call_args[1]["repo_id"] == "steve/drep"
    assert call_args[1]["commit_sha"] == "abc123"


@pytest.mark.asyncio
async def test_post_review_creates_comments():
    """Test post_review() posts summary and inline comments via the anchor."""
    from drep.models.pr_review_findings import PRReviewResult, ReviewComment
    from drep.pr_review.analyzer import PreparedReview, PRReviewAnalyzer

    gitea = AsyncMock()
    llm = AsyncMock()

    analyzer = PRReviewAnalyzer(llm, gitea)

    anchor = _anchor()
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
    prepared = PreparedReview(
        result=result,
        anchor=anchor,
        added_lines={"src/file.py": frozenset({10, 20})},
    )

    await analyzer.post_review(prepared)

    # Should create 1 summary comment
    gitea.create_pr_comment.assert_called_once()
    summary_call = gitea.create_pr_comment.call_args
    body = summary_call.kwargs["body"]
    assert "Overall good PR" in body
    assert "Approve" in body or "✅" in body

    # Should create 2 inline comments
    assert gitea.create_pr_review_comment.call_count == 2

    # Verify inline comment calls use the anchor
    calls = gitea.create_pr_review_comment.call_args_list
    assert calls[0].kwargs["anchor"] is anchor
    assert calls[0].kwargs["line"] == 10
    assert "Fix this" in calls[0].kwargs["body"]
    assert calls[1].kwargs["line"] == 20
    assert "Good job" in calls[1].kwargs["body"]


@pytest.mark.asyncio
async def test_post_review_no_approval():
    """Test post_review() shows 'Needs Changes' when not approved."""
    from drep.models.pr_review_findings import PRReviewResult
    from drep.pr_review.analyzer import PreparedReview, PRReviewAnalyzer

    gitea = AsyncMock()
    llm = AsyncMock()

    analyzer = PRReviewAnalyzer(llm, gitea)

    result = PRReviewResult(
        comments=[],
        summary="Issues found",
        approve=False,
        concerns=["Missing tests"],
    )
    prepared = PreparedReview(result=result, anchor=_anchor(), added_lines={})

    await analyzer.post_review(prepared)

    # Summary should indicate changes needed
    summary_call = gitea.create_pr_comment.call_args
    body = summary_call.kwargs["body"]
    assert "Needs Changes" in body or "🔍" in body or "concerns" in body.lower()


@pytest.mark.asyncio
async def test_review_pr_handles_gitea_error():
    """Test review_pr() handles adapter errors gracefully."""
    from drep.pr_review.analyzer import PRReviewAnalyzer

    gitea = AsyncMock()
    gitea.get_pr.side_effect = ValueError("PR not found")

    llm = AsyncMock()

    analyzer = PRReviewAnalyzer(llm, gitea)

    # Should raise the adapter error
    with pytest.raises(ValueError, match="PR not found"):
        await analyzer.review_pr("steve", "drep", 999)


@pytest.mark.asyncio
async def test_review_pr_handles_llm_error():
    """Test review_pr() handles LLM errors gracefully."""
    from drep.pr_review.analyzer import PRReviewAnalyzer

    gitea = _mock_adapter(diff_text="diff --git a/test.py b/test.py\n")

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
    from drep.pr_review.analyzer import PreparedReview, PRReviewAnalyzer

    gitea = AsyncMock()
    llm = AsyncMock()

    analyzer = PRReviewAnalyzer(llm, gitea)

    result = PRReviewResult(
        comments=[],  # No inline comments
        summary="No issues found",
        approve=True,
        concerns=[],
    )
    prepared = PreparedReview(result=result, anchor=_anchor(), added_lines={})

    await analyzer.post_review(prepared)

    # Should create summary comment
    gitea.create_pr_comment.assert_called_once()

    # Should NOT create inline comments
    gitea.create_pr_review_comment.assert_not_called()


@pytest.mark.asyncio
async def test_truncate_diff_strategy():
    """Test diff truncation keeps first 15k and last 5k chars."""
    from drep.pr_review.analyzer import PRReviewAnalyzer

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
""" + (hunk_lines * 200)  # 200 lines * 106 chars = ~21k chars

    gitea = _mock_adapter(diff_text=large_diff)

    analyzer = PRReviewAnalyzer(llm, gitea)
    await analyzer.review_pr("steve", "drep", 42)

    # Extract the diff content sent to LLM
    call_args = llm.analyze_code_json.call_args
    prompt = call_args[1]["system_prompt"]

    # Should contain TRUNCATED marker
    assert "TRUNCATED" in prompt or "truncated" in prompt.lower()


@pytest.mark.asyncio
async def test_post_review_skips_invalid_line_numbers():
    """Test post_review() skips comments with invalid line numbers."""
    from drep.models.pr_review_findings import PRReviewResult, ReviewComment
    from drep.pr_review.analyzer import PreparedReview, PRReviewAnalyzer

    gitea = AsyncMock()
    llm = AsyncMock()

    analyzer = PRReviewAnalyzer(llm, gitea)

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
    prepared = PreparedReview(
        result=result,
        anchor=_anchor(),
        added_lines={"src/file.py": frozenset({2})},
    )

    await analyzer.post_review(prepared)

    # Should create summary comment
    gitea.create_pr_comment.assert_called_once()

    # Should only create 1 inline comment (the valid one)
    assert gitea.create_pr_review_comment.call_count == 1

    # Verify it was the valid comment that was posted
    call = gitea.create_pr_review_comment.call_args
    assert call.kwargs["line"] == 2
    assert "Valid comment" in call.kwargs["body"]


@pytest.mark.asyncio
async def test_posted_review_validates_against_its_own_diff():
    """C13: posting review A after reviewing B validates against A's diff.

    Previously the analyzer kept one mutable _current_hunks field: reviewing
    PR B replaced A's hunks, so posting A silently validated against B.
    """
    from drep.models.pr_review_findings import PRReviewResult, ReviewComment
    from drep.pr_review.analyzer import PreparedReview, PRReviewAnalyzer

    gitea = AsyncMock()
    llm = AsyncMock()

    analyzer = PRReviewAnalyzer(llm, gitea)

    result_a = PRReviewResult(
        comments=[ReviewComment(file_path="a.py", line=2, severity="info", comment="on A")],
        summary="A",
        approve=True,
        concerns=[],
    )
    prepared_a = PreparedReview(
        result=result_a, anchor=_anchor(pr_number=1), added_lines={"a.py": frozenset({2})}
    )

    # Review B happens between A's review and A's posting
    result_b = PRReviewResult(
        comments=[ReviewComment(file_path="b.py", line=9, severity="info", comment="on B")],
        summary="B",
        approve=True,
        concerns=[],
    )
    prepared_b = PreparedReview(
        result=result_b, anchor=_anchor(pr_number=2), added_lines={"b.py": frozenset({9})}
    )
    await analyzer.post_review(prepared_b)

    # Posting A must still use A's anchor and A's added lines
    await analyzer.post_review(prepared_a)
    call = gitea.create_pr_review_comment.call_args
    assert call.kwargs["anchor"].pr_number == 1
    assert call.kwargs["line"] == 2


def test_is_added_line():
    """PreparedReview.is_added_line() validates against the reviewed diff."""
    from drep.models.pr_review_findings import PRReviewResult
    from drep.pr_review.analyzer import PreparedReview

    prepared = PreparedReview(
        result=PRReviewResult(comments=[], summary="s", approve=True, concerns=[]),
        anchor=_anchor(),
        added_lines={
            "test.py": frozenset({2, 4, 6}),
            "other.py": frozenset({11}),
        },
    )

    # Valid lines (added lines)
    assert prepared.is_added_line("test.py", 2) is True
    assert prepared.is_added_line("test.py", 4) is True
    assert prepared.is_added_line("test.py", 6) is True
    assert prepared.is_added_line("other.py", 11) is True

    # Invalid lines (not added, removed, or non-existent)
    assert prepared.is_added_line("test.py", 1) is False
    assert prepared.is_added_line("test.py", 3) is False
    assert prepared.is_added_line("test.py", 5) is False
    assert prepared.is_added_line("test.py", 999) is False
    assert prepared.is_added_line("nonexistent.py", 1) is False
