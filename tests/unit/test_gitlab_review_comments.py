"""Unit tests for GitLabAdapter inline review comments (anchored positioning)."""

import httpx
import pytest
import respx

from drep.adapters.gitlab import GitLabAdapter

# ===== post_review_comment() Tests =====


@pytest.mark.asyncio
@respx.mock
async def test_post_review_comment_success():
    """Test post_review_comment() successfully posts inline comment."""

    mr_data = {
        "iid": 42,
        "diff_refs": {
            "base_sha": "abc123",
            "head_sha": "def456",
            "start_sha": "abc123",
        },
    }

    # Mock MR retrieval (needed to get diff_refs)
    respx.get("https://gitlab.com/api/v4/projects/owner%2Frepo/merge_requests/42").mock(
        return_value=httpx.Response(200, json=mr_data)
    )

    # Mock successful discussion creation
    respx.post(
        "https://gitlab.com/api/v4/projects/owner%2Frepo/merge_requests/42/discussions"
    ).mock(return_value=httpx.Response(201, json={"id": "discussion123"}))

    adapter = GitLabAdapter("glpat_token")

    try:
        # Should not raise
        await adapter.post_review_comment(
            "owner", "repo", 42, "src/main.py", 15, "Consider refactoring"
        )
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_post_review_comment_sends_position():
    """Test post_review_comment() sends correct position object."""

    mr_data = {
        "iid": 42,
        "diff_refs": {
            "base_sha": "abc123",
            "head_sha": "def456",
            "start_sha": "abc123",
        },
    }

    # Track the request payload
    request_data = {}

    def capture_request(request):
        import json

        request_data["payload"] = json.loads(request.content)
        return httpx.Response(201, json={"id": "discussion123"})

    # Mock MR retrieval
    respx.get("https://gitlab.com/api/v4/projects/owner%2Frepo/merge_requests/42").mock(
        return_value=httpx.Response(200, json=mr_data)
    )

    # Mock discussion creation
    respx.post(
        "https://gitlab.com/api/v4/projects/owner%2Frepo/merge_requests/42/discussions"
    ).mock(side_effect=capture_request)

    adapter = GitLabAdapter("glpat_token")

    try:
        await adapter.post_review_comment("owner", "repo", 42, "src/main.py", 15, "Comment")

        # Verify position object structure
        assert "position" in request_data["payload"]
        position = request_data["payload"]["position"]
        assert position["base_sha"] == "abc123"
        assert position["head_sha"] == "def456"
        assert position["start_sha"] == "abc123"
        assert position["position_type"] == "text"
        assert position["new_path"] == "src/main.py"
        assert position["new_line"] == 15
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_post_review_comment_missing_diff_refs():
    """Test post_review_comment() raises ValueError when diff_refs missing."""

    mr_data = {
        "iid": 42,
        # Missing diff_refs!
    }

    # Mock MR retrieval
    respx.get("https://gitlab.com/api/v4/projects/owner%2Frepo/merge_requests/42").mock(
        return_value=httpx.Response(200, json=mr_data)
    )

    adapter = GitLabAdapter("glpat_token")

    try:
        with pytest.raises(ValueError, match=r"missing 'diff_refs' field"):
            await adapter.post_review_comment("owner", "repo", 42, "src/main.py", 15, "Comment")
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_post_review_comment_400_error():
    """Test post_review_comment() raises ValueError for 400 (invalid position)."""

    mr_data = {
        "iid": 42,
        "diff_refs": {
            "base_sha": "abc123",
            "head_sha": "def456",
            "start_sha": "abc123",
        },
    }

    # Mock MR retrieval
    respx.get("https://gitlab.com/api/v4/projects/owner%2Frepo/merge_requests/42").mock(
        return_value=httpx.Response(200, json=mr_data)
    )

    # Mock 400 error (invalid position)
    respx.post(
        "https://gitlab.com/api/v4/projects/owner%2Frepo/merge_requests/42/discussions"
    ).mock(return_value=httpx.Response(400, text="Invalid position"))

    adapter = GitLabAdapter("glpat_token")

    try:
        with pytest.raises(ValueError, match=r"Invalid position for review comment"):
            await adapter.post_review_comment("owner", "repo", 42, "src/main.py", 99, "Comment")
    finally:
        await adapter.close()
