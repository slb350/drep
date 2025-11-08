"""Unit tests for GitHubAdapter."""

import base64

import httpx
import pytest
import respx


@pytest.mark.asyncio
async def test_github_adapter_inherits_from_base_adapter():
    """Test that GitHubAdapter inherits from BaseAdapter."""
    from drep.adapters.base import BaseAdapter
    from drep.adapters.github import GitHubAdapter

    assert issubclass(GitHubAdapter, BaseAdapter)


@pytest.mark.asyncio
async def test_github_adapter_initialization():
    """Test GitHubAdapter initialization with token."""
    from drep.adapters.github import GitHubAdapter

    token = "ghp_test_token_123"

    adapter = GitHubAdapter(token)

    # Verify URL is set to GitHub API
    assert adapter.url == "https://api.github.com"
    assert adapter.token == token

    # Verify HTTP client is created
    assert adapter.client is not None
    assert isinstance(adapter.client, httpx.AsyncClient)

    # Clean up
    await adapter.close()


@pytest.mark.asyncio
async def test_github_adapter_client_headers():
    """Test that HTTP client has correct authorization header."""
    from drep.adapters.github import GitHubAdapter

    token = "ghp_test_token_abc"
    adapter = GitHubAdapter(token)

    # Check authorization header is set correctly (GitHub uses Bearer)
    assert "Authorization" in adapter.client.headers
    assert adapter.client.headers["Authorization"] == f"Bearer {token}"

    # Check Accept header for GitHub API v3
    assert "Accept" in adapter.client.headers
    assert "application/vnd.github" in adapter.client.headers["Accept"]

    await adapter.close()


@pytest.mark.asyncio
async def test_github_adapter_close():
    """Test that close() properly closes the HTTP client."""
    from drep.adapters.github import GitHubAdapter

    adapter = GitHubAdapter("ghp_token")

    # Client should be open
    assert not adapter.client.is_closed

    # Close the adapter
    await adapter.close()

    # Client should be closed
    assert adapter.client.is_closed


@pytest.mark.asyncio
async def test_github_adapter_timeout():
    """Test that HTTP client has reasonable timeout configured."""
    from drep.adapters.github import GitHubAdapter

    adapter = GitHubAdapter("ghp_token")

    # Check timeout is set (should be 30 seconds as per design)
    assert adapter.client.timeout is not None
    assert adapter.client.timeout.read == 30.0

    await adapter.close()


# ===== create_issue() Tests =====


@pytest.mark.asyncio
@respx.mock
async def test_create_issue_success():
    """Test create_issue() successfully creates issue and returns number."""
    from drep.adapters.github import GitHubAdapter

    # Mock successful issue creation
    respx.post("https://api.github.com/repos/owner/repo/issues").mock(
        return_value=httpx.Response(201, json={"number": 42})
    )

    adapter = GitHubAdapter("ghp_token")

    try:
        issue_number = await adapter.create_issue(
            owner="owner",
            repo="repo",
            title="[Test] Issue title",
            body="Issue body content",
            labels=["documentation", "automated"],
        )
        assert issue_number == 42
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_create_issue_without_labels():
    """Test create_issue() works without labels."""
    from drep.adapters.github import GitHubAdapter

    # Mock successful issue creation
    respx.post("https://api.github.com/repos/owner/repo/issues").mock(
        return_value=httpx.Response(201, json={"number": 43})
    )

    adapter = GitHubAdapter("ghp_token")

    try:
        issue_number = await adapter.create_issue(
            owner="owner", repo="repo", title="[Test] Issue without labels", body="Body content"
        )
        assert issue_number == 43
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_create_issue_sends_correct_payload():
    """Test create_issue() sends correct JSON payload with label names."""
    from drep.adapters.github import GitHubAdapter

    # Track the request payload
    request_data = {}

    def capture_request(request):
        import json

        request_data["payload"] = json.loads(request.content)
        return httpx.Response(201, json={"number": 44})

    respx.post("https://api.github.com/repos/owner/repo/issues").mock(side_effect=capture_request)

    adapter = GitHubAdapter("ghp_token")

    try:
        await adapter.create_issue(
            owner="owner",
            repo="repo",
            title="Test Title",
            body="Test Body",
            labels=["bug", "help wanted"],
        )

        # Verify payload structure - GitHub uses label names (strings), not IDs
        assert request_data["payload"]["title"] == "Test Title"
        assert request_data["payload"]["body"] == "Test Body"
        assert request_data["payload"]["labels"] == ["bug", "help wanted"]
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_create_issue_error_handling():
    """Test create_issue() raises ValueError with response text on error."""
    from drep.adapters.github import GitHubAdapter

    # Mock error response
    respx.post("https://api.github.com/repos/owner/repo/issues").mock(
        return_value=httpx.Response(403, text="Forbidden: Permission denied")
    )

    adapter = GitHubAdapter("ghp_token")

    try:
        with pytest.raises(ValueError, match="Failed to create issue"):
            await adapter.create_issue(owner="owner", repo="repo", title="Test", body="Test")
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_create_issue_empty_labels_works():
    """Test create_issue() works with empty labels."""
    from drep.adapters.github import GitHubAdapter

    # Mock successful issue creation
    respx.post("https://api.github.com/repos/owner/repo/issues").mock(
        return_value=httpx.Response(201, json={"number": 51})
    )

    adapter = GitHubAdapter("ghp_token")

    try:
        issue_number = await adapter.create_issue(
            owner="owner", repo="repo", title="Test", body="Test", labels=[]
        )
        assert issue_number == 51
    finally:
        await adapter.close()


# ===== PR Methods Tests =====


@pytest.mark.asyncio
@respx.mock
async def test_get_pr_success():
    """Test get_pr() successfully fetches PR details."""
    from drep.adapters.github import GitHubAdapter

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
    from drep.adapters.github import GitHubAdapter

    # Mock 404 response
    respx.get("https://api.github.com/repos/owner/repo/pulls/999").mock(
        return_value=httpx.Response(404, json={"message": "Not Found"})
    )

    adapter = GitHubAdapter("ghp_token")

    try:
        with pytest.raises(ValueError, match="Pull request #999 not found"):
            await adapter.get_pr("owner", "repo", 999)
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_get_pr_diff_success():
    """Test get_pr_diff() successfully fetches PR diff."""
    from drep.adapters.github import GitHubAdapter

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
    from drep.adapters.github import GitHubAdapter

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
    from drep.adapters.github import GitHubAdapter

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
    from drep.adapters.github import GitHubAdapter

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
    from drep.adapters.github import GitHubAdapter

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
    from drep.adapters.github import GitHubAdapter

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
    from drep.adapters.github import GitHubAdapter

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
        with pytest.raises(ValueError, match="Failed to create review comment"):
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


# ===== get_file_content() Tests =====


@pytest.mark.asyncio
@respx.mock
async def test_get_file_content_success():
    """Test get_file_content() successfully fetches file content."""
    from drep.adapters.github import GitHubAdapter

    # GitHub returns base64-encoded content
    content = "def hello():\n    print('Hello, world!')\n"
    content_b64 = base64.b64encode(content.encode("utf-8")).decode("utf-8")

    respx.get("https://api.github.com/repos/owner/repo/contents/src/hello.py").mock(
        return_value=httpx.Response(200, json={"content": content_b64, "encoding": "base64"})
    )

    adapter = GitHubAdapter("ghp_token")

    try:
        result = await adapter.get_file_content("owner", "repo", "src/hello.py", "main")
        assert result == content
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_get_file_content_with_ref():
    """Test get_file_content() uses correct ref parameter."""
    from drep.adapters.github import GitHubAdapter

    content = "# README\n"
    content_b64 = base64.b64encode(content.encode("utf-8")).decode("utf-8")

    # Capture request to verify ref parameter
    request_data = {}

    def capture_request(request):
        request_data["params"] = dict(request.url.params)
        return httpx.Response(200, json={"content": content_b64})

    respx.get("https://api.github.com/repos/owner/repo/contents/README.md").mock(
        side_effect=capture_request
    )

    adapter = GitHubAdapter("ghp_token")

    try:
        await adapter.get_file_content("owner", "repo", "README.md", "feature-branch")
        assert request_data["params"]["ref"] == "feature-branch"
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_get_file_content_empty_file():
    """Test get_file_content() handles empty files."""
    from drep.adapters.github import GitHubAdapter

    # Empty file - base64 of empty string
    content_b64 = base64.b64encode(b"").decode("utf-8")

    respx.get("https://api.github.com/repos/owner/repo/contents/empty.txt").mock(
        return_value=httpx.Response(200, json={"content": content_b64})
    )

    adapter = GitHubAdapter("ghp_token")

    try:
        result = await adapter.get_file_content("owner", "repo", "empty.txt", "main")
        assert result == ""
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_get_file_content_not_found():
    """Test get_file_content() raises ValueError for non-existent file."""
    from drep.adapters.github import GitHubAdapter

    respx.get("https://api.github.com/repos/owner/repo/contents/nonexistent.py").mock(
        return_value=httpx.Response(404, json={"message": "Not Found"})
    )

    adapter = GitHubAdapter("ghp_token")

    try:
        with pytest.raises(ValueError, match="File nonexistent.py not found"):
            await adapter.get_file_content("owner", "repo", "nonexistent.py", "main")
    finally:
        await adapter.close()
