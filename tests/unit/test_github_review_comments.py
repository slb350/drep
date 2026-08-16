"""GitHubAdapter anchored inline review comment tests."""

import httpx
import pytest
import respx

from drep.adapters.base import ReviewAnchor
from drep.adapters.github import GitHubAdapter

# ===== create_pr_review_comment() Tests =====


@pytest.mark.asyncio
@respx.mock
async def test_create_pr_review_comment_success():
    """Test create_pr_review_comment() successfully posts inline comment with commit_sha."""

    # Mock review comment creation (no get_pr needed - commit_sha provided)
    respx.post("https://api.github.com/repos/owner/repo/pulls/42/comments").mock(
        return_value=httpx.Response(201, json={"id": 456})
    )

    adapter = GitHubAdapter("ghp_token")

    try:
        # Should not raise - anchor provided directly, no get_pr fetch
        await adapter.create_pr_review_comment(
            anchor=ReviewAnchor(
                owner="owner", repo="repo", pr_number=42, commit_sha="abc123def456"
            ),
            file_path="src/module.py",
            line=15,
            body="Consider adding error handling here",
        )
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_create_pr_review_comment_sends_correct_payload():
    """Test create_pr_review_comment() sends correct JSON payload with provided commit_sha."""

    request_data = {}

    def capture_request(request):
        import json

        request_data["payload"] = json.loads(request.content)
        return httpx.Response(201, json={"id": 456})

    respx.post("https://api.github.com/repos/owner/repo/pulls/42/comments").mock(
        side_effect=capture_request
    )

    adapter = GitHubAdapter("ghp_token")

    try:
        await adapter.create_pr_review_comment(
            anchor=ReviewAnchor(owner="owner", repo="repo", pr_number=42, commit_sha="xyz789abc"),
            file_path="src/module.py",
            line=15,
            body="Consider adding error handling here",
        )

        # Verify payload uses the anchor's commit_sha (not fetched from PR)
        payload = request_data["payload"]
        assert payload["commit_id"] == "xyz789abc"  # Uses provided SHA
        assert payload["path"] == "src/module.py"
        assert payload["line"] == 15
        assert payload["body"] == "Consider adding error handling here"
        assert payload["side"] == "RIGHT"  # GitHub requires side field
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_create_pr_review_comment_error_handling():
    """Test create_pr_review_comment() raises ValueError on error."""

    respx.post("https://api.github.com/repos/owner/repo/pulls/42/comments").mock(
        return_value=httpx.Response(403, text="Forbidden: Permission denied")
    )

    adapter = GitHubAdapter("ghp_token")

    try:
        with pytest.raises(ValueError, match=r"Failed to create review comment"):
            await adapter.create_pr_review_comment(
                anchor=ReviewAnchor(owner="owner", repo="repo", pr_number=42, commit_sha="abc123"),
                file_path="test.py",
                line=10,
                body="Comment",
            )
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_create_pr_review_comment_handles_422_validation():
    """Test create_pr_review_comment() handles 422 validation error for invalid line."""

    respx.post("https://api.github.com/repos/owner/repo/pulls/42/comments").mock(
        return_value=httpx.Response(
            422, json={"message": "Validation Failed", "errors": [{"message": "Invalid line"}]}
        )
    )

    adapter = GitHubAdapter("ghp_token")

    try:
        # C1: the 422 handling from post_review_comment is now in the canonical
        # method, producing the specific invalid-line error
        with pytest.raises(ValueError, match=r"Invalid line number 999"):
            await adapter.create_pr_review_comment(
                anchor=ReviewAnchor(owner="owner", repo="repo", pr_number=42, commit_sha="abc123"),
                file_path="test.py",
                line=999,
                body="Comment",
            )
    finally:
        await adapter.close()
