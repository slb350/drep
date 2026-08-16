"""GitHubAdapter PR data tests: get_pr, get_pr_diff, create_pr_comment, post_review_comment."""

import httpx
import pytest
import respx

from drep.adapters.github import GitHubAdapter

# ===== PR Methods Tests =====


@pytest.mark.asyncio
@respx.mock
async def test_get_pr_success():
    """Test get_pr() successfully fetches PR details."""

    # Mock PR response
    pr_data = {
        "number": 42,
        "title": "Add feature X",
        "body": "This PR adds feature X",
        "state": "open",
        "base": {"ref": "main"},
        "head": {"ref": "feature-x", "sha": "abc123def456"},
        "user": {"login": "developer"},
    }

    respx.get("https://api.github.com/repos/owner/repo/pulls/42").mock(
        return_value=httpx.Response(200, json=pr_data)
    )

    adapter = GitHubAdapter("ghp_token")

    try:
        result = await adapter.get_pr("owner", "repo", 42)
        assert result["number"] == 42
        assert result["title"] == "Add feature X"
        assert result["state"] == "open"
        assert result["head"]["sha"] == "abc123def456"
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_get_pr_not_found():
    """Test get_pr() raises ValueError for non-existent PR."""

    # Mock 404 response
    respx.get("https://api.github.com/repos/owner/repo/pulls/999").mock(
        return_value=httpx.Response(404, json={"message": "Not Found"})
    )

    adapter = GitHubAdapter("ghp_token")

    try:
        with pytest.raises(ValueError, match=r"Pull request #999 not found"):
            await adapter.get_pr("owner", "repo", 999)
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_get_pr_diff_success():
    """Test get_pr_diff() successfully fetches PR diff."""

    diff_content = """diff --git a/src/module.py b/src/module.py
index abc123..def456 100644
--- a/src/module.py
+++ b/src/module.py
@@ -10,7 +10,9 @@ def calculate(x, y):
     \"\"\"Calculate sum.\"\"\"
-    return x + y
+    result = x + y
+    logger.info(f"Calculated: {result}")
+    return result
"""

    respx.get("https://api.github.com/repos/owner/repo/pulls/42").mock(
        return_value=httpx.Response(
            200, text=diff_content, headers={"Content-Type": "application/vnd.github.v3.diff"}
        )
    )

    adapter = GitHubAdapter("ghp_token")

    try:
        diff = await adapter.get_pr_diff("owner", "repo", 42)
        assert "diff --git" in diff
        assert "src/module.py" in diff
        assert "+    result = x + y" in diff
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_get_pr_diff_large():
    """Test get_pr_diff() handles large diffs (no size limit at this layer)."""

    # Create a large diff (> 100KB)
    large_diff = "diff --git a/file.py b/file.py\n" + ("+" + "x" * 1000 + "\n") * 200

    respx.get("https://api.github.com/repos/owner/repo/pulls/42").mock(
        return_value=httpx.Response(
            200, text=large_diff, headers={"Content-Type": "application/vnd.github.v3.diff"}
        )
    )

    adapter = GitHubAdapter("ghp_token")

    try:
        diff = await adapter.get_pr_diff("owner", "repo", 42)
        assert len(diff) > 100000
        assert "diff --git" in diff
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_create_pr_comment_success():
    """Test create_pr_comment() successfully posts general comment."""

    respx.post("https://api.github.com/repos/owner/repo/issues/42/comments").mock(
        return_value=httpx.Response(201, json={"id": 123, "body": "Test comment"})
    )

    adapter = GitHubAdapter("ghp_token")

    try:
        # Should not raise
        await adapter.create_pr_comment("owner", "repo", 42, "Test comment")
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_create_pr_comment_sends_correct_payload():
    """Test create_pr_comment() sends correct JSON payload."""

    request_data = {}

    def capture_request(request):
        import json

        request_data["payload"] = json.loads(request.content)
        return httpx.Response(201, json={"id": 123})

    respx.post("https://api.github.com/repos/owner/repo/issues/42/comments").mock(
        side_effect=capture_request
    )

    adapter = GitHubAdapter("ghp_token")

    try:
        await adapter.create_pr_comment("owner", "repo", 42, "Review summary comment")

        # Verify payload
        assert request_data["payload"]["body"] == "Review summary comment"
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_post_review_comment_success():
    """Test post_review_comment() successfully posts inline comment."""

    # Mock get_pr to get commit SHA
    pr_data = {
        "number": 42,
        "head": {"sha": "abc123def456"},
    }
    respx.get("https://api.github.com/repos/owner/repo/pulls/42").mock(
        return_value=httpx.Response(200, json=pr_data)
    )

    # Mock review comment creation
    respx.post("https://api.github.com/repos/owner/repo/pulls/42/comments").mock(
        return_value=httpx.Response(201, json={"id": 456})
    )

    adapter = GitHubAdapter("ghp_token")

    try:
        # Should not raise
        await adapter.post_review_comment(
            owner="owner",
            repo="repo",
            pr_number=42,
            file_path="src/module.py",
            line=15,
            body="Consider adding error handling here",
        )
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_post_review_comment_sends_correct_payload():
    """Test post_review_comment() sends correct JSON payload."""

    # Mock get_pr to get commit SHA
    pr_data = {
        "number": 42,
        "head": {"sha": "abc123def456"},
    }
    respx.get("https://api.github.com/repos/owner/repo/pulls/42").mock(
        return_value=httpx.Response(200, json=pr_data)
    )

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
        await adapter.post_review_comment(
            owner="owner",
            repo="repo",
            pr_number=42,
            file_path="src/module.py",
            line=15,
            body="Consider adding error handling here",
        )

        # Verify payload structure for GitHub API
        payload = request_data["payload"]
        assert payload["commit_id"] == "abc123def456"
        assert payload["path"] == "src/module.py"
        assert payload["line"] == 15
        assert payload["body"] == "Consider adding error handling here"
        assert payload["side"] == "RIGHT"  # GitHub requires side field
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_post_review_comment_error_handling():
    """Test post_review_comment() raises ValueError on error."""

    # Mock get_pr to get commit SHA
    pr_data = {
        "number": 42,
        "head": {"sha": "abc123def456"},
    }
    respx.get("https://api.github.com/repos/owner/repo/pulls/42").mock(
        return_value=httpx.Response(200, json=pr_data)
    )

    respx.post("https://api.github.com/repos/owner/repo/pulls/42/comments").mock(
        return_value=httpx.Response(403, text="Forbidden: Permission denied")
    )

    adapter = GitHubAdapter("ghp_token")

    try:
        with pytest.raises(ValueError, match=r"Failed to create review comment"):
            await adapter.post_review_comment(
                owner="owner",
                repo="repo",
                pr_number=42,
                file_path="test.py",
                line=10,
                body="Comment",
            )
    finally:
        await adapter.close()
