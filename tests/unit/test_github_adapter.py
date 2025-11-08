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


# ===== create_pr_review_comment() Tests =====


@pytest.mark.asyncio
@respx.mock
async def test_create_pr_review_comment_success():
    """Test create_pr_review_comment() successfully posts inline comment with commit_sha."""
    from drep.adapters.github import GitHubAdapter

    # Mock review comment creation (no get_pr needed - commit_sha provided)
    respx.post("https://api.github.com/repos/owner/repo/pulls/42/comments").mock(
        return_value=httpx.Response(201, json={"id": 456})
    )

    adapter = GitHubAdapter("ghp_token")

    try:
        # Should not raise - commit_sha provided directly
        await adapter.create_pr_review_comment(
            owner="owner",
            repo="repo",
            pr_number=42,
            commit_sha="abc123def456",
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
    from drep.adapters.github import GitHubAdapter

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
            owner="owner",
            repo="repo",
            pr_number=42,
            commit_sha="xyz789abc",
            file_path="src/module.py",
            line=15,
            body="Consider adding error handling here",
        )

        # Verify payload uses provided commit_sha (not fetched from PR)
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
    from drep.adapters.github import GitHubAdapter

    respx.post("https://api.github.com/repos/owner/repo/pulls/42/comments").mock(
        return_value=httpx.Response(403, text="Forbidden: Permission denied")
    )

    adapter = GitHubAdapter("ghp_token")

    try:
        with pytest.raises(ValueError, match="Failed to create review comment"):
            await adapter.create_pr_review_comment(
                owner="owner",
                repo="repo",
                pr_number=42,
                commit_sha="abc123",
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
    from drep.adapters.github import GitHubAdapter

    respx.post("https://api.github.com/repos/owner/repo/pulls/42/comments").mock(
        return_value=httpx.Response(
            422, json={"message": "Validation Failed", "errors": [{"message": "Invalid line"}]}
        )
    )

    adapter = GitHubAdapter("ghp_token")

    try:
        with pytest.raises(ValueError, match="Failed to create review comment"):
            await adapter.create_pr_review_comment(
                owner="owner",
                repo="repo",
                pr_number=42,
                commit_sha="abc123",
                file_path="test.py",
                line=999,
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


# ===== GitHub Enterprise Tests =====


@pytest.mark.asyncio
@respx.mock
async def test_create_issue_with_enterprise_url():
    """Test create_issue() works with GitHub Enterprise custom URL."""
    from drep.adapters.github import GitHubAdapter

    enterprise_url = "https://github.company.com/api/v3"
    respx.post(f"{enterprise_url}/repos/owner/repo/issues").mock(
        return_value=httpx.Response(201, json={"number": 42})
    )

    adapter = GitHubAdapter("ghp_token", url=enterprise_url)

    try:
        issue_number = await adapter.create_issue(
            "owner", "repo", "Test Issue", "Test Body", ["bug"]
        )
        assert issue_number == 42
    finally:
        await adapter.close()


# ===== Base64 with Newlines Tests =====


@pytest.mark.asyncio
@respx.mock
async def test_get_file_content_base64_with_newlines():
    """Test get_file_content() handles base64 content with embedded newlines."""
    from drep.adapters.github import GitHubAdapter

    # Create file content
    content = "def hello():\n    print('Hello, world!')\n"

    # Encode to base64
    b64_clean = base64.b64encode(content.encode()).decode()

    # Split base64 into lines (GitHub does this for large files)
    b64_with_newlines = "\n".join([b64_clean[i : i + 60] for i in range(0, len(b64_clean), 60)])

    respx.get("https://api.github.com/repos/owner/repo/contents/test.py").mock(
        return_value=httpx.Response(200, json={"content": b64_with_newlines})
    )

    adapter = GitHubAdapter("ghp_token")

    try:
        result = await adapter.get_file_content("owner", "repo", "test.py", "main")
        assert result == content
    finally:
        await adapter.close()


# ===== HTTP Error Code Tests =====


@pytest.mark.asyncio
@respx.mock
async def test_create_issue_unauthorized_401():
    """Test create_issue() handles 401 unauthorized error."""
    from drep.adapters.github import GitHubAdapter

    respx.post("https://api.github.com/repos/owner/repo/issues").mock(
        return_value=httpx.Response(401, json={"message": "Bad credentials"})
    )

    adapter = GitHubAdapter("ghp_token")

    try:
        with pytest.raises(ValueError, match="Failed to create issue"):
            await adapter.create_issue("owner", "repo", "Test", "Test")
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_post_review_comment_validation_failed_422():
    """Test post_review_comment() handles 422 validation error for invalid line."""
    from drep.adapters.github import GitHubAdapter

    # Mock get_pr
    pr_data = {"number": 42, "head": {"sha": "abc123"}}
    respx.get("https://api.github.com/repos/owner/repo/pulls/42").mock(
        return_value=httpx.Response(200, json=pr_data)
    )

    # Mock 422 error for invalid line
    respx.post("https://api.github.com/repos/owner/repo/pulls/42/comments").mock(
        return_value=httpx.Response(
            422, json={"message": "Validation Failed", "errors": [{"field": "line"}]}
        )
    )

    adapter = GitHubAdapter("ghp_token")

    try:
        with pytest.raises(ValueError, match="Invalid line number.*Line must be part of PR diff"):
            await adapter.post_review_comment("owner", "repo", 42, "test.py", 999, "Comment")
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_create_issue_server_error_500():
    """Test create_issue() handles 500 server error."""
    from drep.adapters.github import GitHubAdapter

    respx.post("https://api.github.com/repos/owner/repo/issues").mock(
        return_value=httpx.Response(500, json={"message": "Internal Server Error"})
    )

    adapter = GitHubAdapter("ghp_token")

    try:
        with pytest.raises(ValueError, match="Failed to create issue"):
            await adapter.create_issue("owner", "repo", "Test", "Test")
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_get_pr_rate_limit_403():
    """Test get_pr() detects and reports rate limit errors."""
    from drep.adapters.github import GitHubAdapter

    respx.get("https://api.github.com/repos/owner/repo/pulls/42").mock(
        return_value=httpx.Response(
            403,
            json={"message": "API rate limit exceeded"},
            headers={"X-RateLimit-Remaining": "0", "X-RateLimit-Reset": "1640000000"},
        )
    )

    adapter = GitHubAdapter("ghp_token")

    try:
        with pytest.raises(ValueError, match="GitHub API rate limit exceeded.*Resets at"):
            await adapter.get_pr("owner", "repo", 42)
    finally:
        await adapter.close()


# ===== Constructor Validation Tests =====


def test_github_adapter_empty_token_raises_error():
    """Test that GitHubAdapter raises ValueError for empty token."""
    from drep.adapters.github import GitHubAdapter

    with pytest.raises(ValueError, match="GitHub token cannot be empty"):
        GitHubAdapter("")


def test_github_adapter_whitespace_token_raises_error():
    """Test that GitHubAdapter raises ValueError for whitespace-only token."""
    from drep.adapters.github import GitHubAdapter

    with pytest.raises(ValueError, match="GitHub token cannot be empty"):
        GitHubAdapter("   ")


def test_github_adapter_invalid_url_raises_error():
    """Test that GitHubAdapter raises ValueError for invalid URL."""
    from drep.adapters.github import GitHubAdapter

    with pytest.raises(ValueError, match="GitHub URL must start with http"):
        GitHubAdapter("ghp_token", url="ftp://invalid.com")


# ===== Network Error Tests =====


@pytest.mark.asyncio
@respx.mock
async def test_create_issue_timeout_error():
    """Test create_issue() handles timeout errors gracefully."""
    from drep.adapters.github import GitHubAdapter

    def timeout_handler(request):
        raise httpx.TimeoutException("Request timeout")

    respx.post("https://api.github.com/repos/owner/repo/issues").mock(side_effect=timeout_handler)

    adapter = GitHubAdapter("ghp_token")

    try:
        with pytest.raises(ValueError, match="GitHub API request timed out"):
            await adapter.create_issue("owner", "repo", "Test", "Test")
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_get_pr_connection_error():
    """Test get_pr() handles connection errors gracefully."""
    from drep.adapters.github import GitHubAdapter

    def connection_error_handler(request):
        raise httpx.ConnectError("Connection failed")

    respx.get("https://api.github.com/repos/owner/repo/pulls/42").mock(
        side_effect=connection_error_handler
    )

    adapter = GitHubAdapter("ghp_token")

    try:
        with pytest.raises(ValueError, match="Cannot connect to GitHub API"):
            await adapter.get_pr("owner", "repo", 42)
    finally:
        await adapter.close()


# ===== Base64 Decode Error Tests =====


@pytest.mark.asyncio
@respx.mock
async def test_get_file_content_invalid_base64():
    """Test get_file_content() handles corrupted base64 gracefully."""
    from drep.adapters.github import GitHubAdapter

    # Invalid base64 content
    respx.get("https://api.github.com/repos/owner/repo/contents/corrupted.py").mock(
        return_value=httpx.Response(200, json={"content": "!@#$%^&*()INVALID"})
    )

    adapter = GitHubAdapter("ghp_token")

    try:
        with pytest.raises(ValueError, match="Failed to decode file content.*invalid base64"):
            await adapter.get_file_content("owner", "repo", "corrupted.py", "main")
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_get_file_content_binary_file():
    """Test get_file_content() rejects binary files gracefully."""
    from drep.adapters.github import GitHubAdapter

    # Binary content (non-UTF8)
    binary_data = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR"  # PNG header
    b64_binary = base64.b64encode(binary_data).decode()

    respx.get("https://api.github.com/repos/owner/repo/contents/image.png").mock(
        return_value=httpx.Response(200, json={"content": b64_binary})
    )

    adapter = GitHubAdapter("ghp_token")

    try:
        with pytest.raises(ValueError, match="binary or non-UTF8.*only supports text files"):
            await adapter.get_file_content("owner", "repo", "image.png", "main")
    finally:
        await adapter.close()


# ===== JSON Parsing Error Tests =====


@pytest.mark.asyncio
@respx.mock
async def test_create_issue_invalid_json_response():
    """Test create_issue() handles non-JSON responses gracefully."""
    from drep.adapters.github import GitHubAdapter

    # Return HTML error page instead of JSON
    respx.post("https://api.github.com/repos/owner/repo/issues").mock(
        return_value=httpx.Response(200, text="<html><body>Error</body></html>")
    )

    adapter = GitHubAdapter("ghp_token")

    try:
        with pytest.raises(ValueError, match="GitHub API returned invalid JSON"):
            await adapter.create_issue("owner", "repo", "Test", "Test")
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_create_issue_missing_number_field():
    """Test create_issue() validates required fields in response."""
    from drep.adapters.github import GitHubAdapter

    # Response missing 'number' field
    respx.post("https://api.github.com/repos/owner/repo/issues").mock(
        return_value=httpx.Response(201, json={"id": 12345, "title": "Test"})
    )

    adapter = GitHubAdapter("ghp_token")

    try:
        with pytest.raises(ValueError, match="missing 'number' field"):
            await adapter.create_issue("owner", "repo", "Test", "Test")
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_post_review_comment_missing_head_sha():
    """Test post_review_comment() validates nested PR structure."""
    from drep.adapters.github import GitHubAdapter

    # PR response missing head.sha
    pr_data = {"number": 42, "head": {}}  # Missing 'sha' field
    respx.get("https://api.github.com/repos/owner/repo/pulls/42").mock(
        return_value=httpx.Response(200, json=pr_data)
    )

    adapter = GitHubAdapter("ghp_token")

    try:
        with pytest.raises(ValueError, match="missing required 'head.sha' field"):
            await adapter.post_review_comment("owner", "repo", 42, "test.py", 10, "Comment")
    finally:
        await adapter.close()


@pytest.mark.asyncio
async def test_close_propagates_keyboard_interrupt():
    """Test that close() propagates KeyboardInterrupt instead of swallowing it."""
    from unittest.mock import AsyncMock

    from drep.adapters.github import GitHubAdapter

    adapter = GitHubAdapter("ghp_token")

    # Mock aclose() to raise KeyboardInterrupt
    adapter.client.aclose = AsyncMock(side_effect=KeyboardInterrupt("User interrupted"))

    # KeyboardInterrupt should propagate
    with pytest.raises(KeyboardInterrupt):
        await adapter.close()


@pytest.mark.asyncio
async def test_close_propagates_system_exit():
    """Test that close() propagates SystemExit instead of swallowing it."""
    from unittest.mock import AsyncMock

    from drep.adapters.github import GitHubAdapter

    adapter = GitHubAdapter("ghp_token")

    # Mock aclose() to raise SystemExit
    adapter.client.aclose = AsyncMock(side_effect=SystemExit(1))

    # SystemExit should propagate
    with pytest.raises(SystemExit):
        await adapter.close()


@pytest.mark.asyncio
async def test_close_suppresses_non_critical_errors():
    """Test that close() suppresses non-critical errors like CloseError."""
    from unittest.mock import AsyncMock

    from drep.adapters.github import GitHubAdapter

    adapter = GitHubAdapter("ghp_token")

    # Mock aclose() to raise a non-critical error
    adapter.client.aclose = AsyncMock(side_effect=RuntimeError("Connection already closed"))

    # Should not raise - error should be suppressed and logged
    await adapter.close()  # Should complete without exception


@pytest.mark.asyncio
async def test_close_propagates_asyncio_cancelled_error():
    """Test that close() propagates asyncio.CancelledError instead of swallowing it."""
    import asyncio
    from unittest.mock import AsyncMock

    from drep.adapters.github import GitHubAdapter

    adapter = GitHubAdapter("ghp_token")

    # Mock aclose() to raise CancelledError
    adapter.client.aclose = AsyncMock(side_effect=asyncio.CancelledError())

    # CancelledError should propagate
    with pytest.raises(asyncio.CancelledError):
        await adapter.close()


@pytest.mark.asyncio
async def test_check_rate_limit_with_zero():
    """Test _check_rate_limit() detects rate limit with '0' string."""
    from drep.adapters.github import GitHubAdapter

    adapter = GitHubAdapter("ghp_token")
    response = httpx.Response(
        403, headers={"X-RateLimit-Remaining": "0", "X-RateLimit-Reset": "1640000000"}
    )

    with pytest.raises(ValueError, match="rate limit exceeded.*Resets at"):
        adapter._check_rate_limit(response, "owner", "repo")

    await adapter.close()


@pytest.mark.asyncio
async def test_check_rate_limit_with_whitespace():
    """Test _check_rate_limit() handles whitespace in header value."""
    from drep.adapters.github import GitHubAdapter

    adapter = GitHubAdapter("ghp_token")
    response = httpx.Response(
        403, headers={"X-RateLimit-Remaining": " 0 ", "X-RateLimit-Reset": "1640000000"}
    )

    with pytest.raises(ValueError, match="rate limit exceeded"):
        adapter._check_rate_limit(response, "owner", "repo")

    await adapter.close()


@pytest.mark.asyncio
async def test_check_rate_limit_with_float():
    """Test _check_rate_limit() handles float string like '0.0'."""
    from drep.adapters.github import GitHubAdapter

    adapter = GitHubAdapter("ghp_token")
    response = httpx.Response(
        403, headers={"X-RateLimit-Remaining": "0.0", "X-RateLimit-Reset": "1640000000"}
    )

    with pytest.raises(ValueError, match="rate limit exceeded"):
        adapter._check_rate_limit(response, "owner", "repo")

    await adapter.close()


@pytest.mark.asyncio
async def test_check_rate_limit_non_zero_remaining():
    """Test _check_rate_limit() doesn't raise when requests remain."""
    from drep.adapters.github import GitHubAdapter

    adapter = GitHubAdapter("ghp_token")
    response = httpx.Response(
        403, headers={"X-RateLimit-Remaining": "100", "X-RateLimit-Reset": "1640000000"}
    )

    # Should not raise - still have requests remaining
    adapter._check_rate_limit(response, "owner", "repo")

    await adapter.close()


@pytest.mark.asyncio
async def test_check_rate_limit_non_403_status():
    """Test _check_rate_limit() ignores non-403 status codes."""
    from drep.adapters.github import GitHubAdapter

    adapter = GitHubAdapter("ghp_token")
    response = httpx.Response(
        404, headers={"X-RateLimit-Remaining": "0", "X-RateLimit-Reset": "1640000000"}
    )

    # Should not raise - not a 403
    adapter._check_rate_limit(response, "owner", "repo")

    await adapter.close()


@pytest.mark.asyncio
async def test_check_rate_limit_missing_header():
    """Test _check_rate_limit() handles missing rate limit header."""
    from drep.adapters.github import GitHubAdapter

    adapter = GitHubAdapter("ghp_token")
    response = httpx.Response(403, headers={})

    # Should not raise - no rate limit header
    adapter._check_rate_limit(response, "owner", "repo")

    await adapter.close()


@pytest.mark.asyncio
async def test_check_rate_limit_invalid_header_value():
    """Test _check_rate_limit() handles invalid (non-numeric) header value."""
    from drep.adapters.github import GitHubAdapter

    adapter = GitHubAdapter("ghp_token")
    response = httpx.Response(
        403, headers={"X-RateLimit-Remaining": "invalid", "X-RateLimit-Reset": "1640000000"}
    )

    # Should not raise - can't parse, so not a valid rate limit response
    adapter._check_rate_limit(response, "owner", "repo")

    await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_get_file_content_missing_content_field():
    """Test get_file_content() validates 'content' field exists in API response."""
    from drep.adapters.github import GitHubAdapter

    # API response missing 'content' field (malformed response)
    respx.get("https://api.github.com/repos/owner/repo/contents/test.py").mock(
        return_value=httpx.Response(
            200, json={"name": "test.py", "path": "test.py", "type": "file"}  # Missing 'content'
        )
    )

    adapter = GitHubAdapter("ghp_token")

    try:
        with pytest.raises(ValueError, match="missing 'content' field"):
            await adapter.get_file_content("owner", "repo", "test.py", "main")
    finally:
        await adapter.close()


# ===== Repository Methods Tests =====


@pytest.mark.asyncio
@respx.mock
async def test_get_default_branch_success():
    """Test get_default_branch() returns default branch name."""
    from drep.adapters.github import GitHubAdapter

    # Mock repository API response with default_branch field
    respx.get("https://api.github.com/repos/owner/repo").mock(
        return_value=httpx.Response(
            200,
            json={
                "name": "repo",
                "owner": {"login": "owner"},
                "default_branch": "main",
                "private": False,
            },
        )
    )

    adapter = GitHubAdapter("ghp_token")

    try:
        branch = await adapter.get_default_branch("owner", "repo")
        assert branch == "main"
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_get_default_branch_master():
    """Test get_default_branch() handles 'master' as default branch."""
    from drep.adapters.github import GitHubAdapter

    # Some repos use 'master' instead of 'main'
    respx.get("https://api.github.com/repos/owner/legacy-repo").mock(
        return_value=httpx.Response(
            200,
            json={
                "name": "legacy-repo",
                "owner": {"login": "owner"},
                "default_branch": "master",
            },
        )
    )

    adapter = GitHubAdapter("ghp_token")

    try:
        branch = await adapter.get_default_branch("owner", "legacy-repo")
        assert branch == "master"
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_get_default_branch_custom_name():
    """Test get_default_branch() handles custom branch names."""
    from drep.adapters.github import GitHubAdapter

    # Some repos use custom default branch names
    respx.get("https://api.github.com/repos/owner/custom-repo").mock(
        return_value=httpx.Response(
            200,
            json={
                "name": "custom-repo",
                "owner": {"login": "owner"},
                "default_branch": "develop",
            },
        )
    )

    adapter = GitHubAdapter("ghp_token")

    try:
        branch = await adapter.get_default_branch("owner", "custom-repo")
        assert branch == "develop"
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_get_default_branch_not_found():
    """Test get_default_branch() raises ValueError when repo not found."""
    from drep.adapters.github import GitHubAdapter

    # Mock 404 response for non-existent repository
    respx.get("https://api.github.com/repos/owner/nonexistent").mock(
        return_value=httpx.Response(404, json={"message": "Not Found"})
    )

    adapter = GitHubAdapter("ghp_token")

    try:
        with pytest.raises(ValueError, match="Repository owner/nonexistent not found"):
            await adapter.get_default_branch("owner", "nonexistent")
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_get_default_branch_missing_field():
    """Test get_default_branch() validates 'default_branch' field exists."""
    from drep.adapters.github import GitHubAdapter

    # Mock malformed API response missing 'default_branch' field
    respx.get("https://api.github.com/repos/owner/repo").mock(
        return_value=httpx.Response(
            200, json={"name": "repo", "owner": {"login": "owner"}}  # Missing default_branch
        )
    )

    adapter = GitHubAdapter("ghp_token")

    try:
        with pytest.raises(ValueError, match="missing 'default_branch' field"):
            await adapter.get_default_branch("owner", "repo")
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_get_default_branch_timeout():
    """Test get_default_branch() handles timeout errors."""
    from drep.adapters.github import GitHubAdapter

    # Mock timeout
    respx.get("https://api.github.com/repos/owner/repo").mock(
        side_effect=httpx.TimeoutException("Request timed out")
    )

    adapter = GitHubAdapter("ghp_token")

    try:
        with pytest.raises(ValueError, match="timed out"):
            await adapter.get_default_branch("owner", "repo")
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_get_default_branch_invalid_json():
    """Test get_default_branch() handles invalid JSON response."""
    from drep.adapters.github import GitHubAdapter

    # Mock response with invalid JSON
    respx.get("https://api.github.com/repos/owner/repo").mock(
        return_value=httpx.Response(200, text="not json")
    )

    adapter = GitHubAdapter("ghp_token")

    try:
        with pytest.raises(ValueError, match="invalid JSON"):
            await adapter.get_default_branch("owner", "repo")
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_get_default_branch_connect_error():
    """Test get_default_branch() handles connection failures."""
    from drep.adapters.github import GitHubAdapter

    # Mock connection error
    respx.get("https://api.github.com/repos/owner/repo").mock(
        side_effect=httpx.ConnectError("Failed to connect")
    )

    adapter = GitHubAdapter("ghp_token")

    try:
        with pytest.raises(ValueError, match="Cannot connect to GitHub API"):
            await adapter.get_default_branch("owner", "repo")
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_get_default_branch_rate_limit_exceeded():
    """Test get_default_branch() detects and reports rate limit errors."""
    from drep.adapters.github import GitHubAdapter

    # Mock rate limit exceeded response
    respx.get("https://api.github.com/repos/owner/repo").mock(
        return_value=httpx.Response(
            403,
            headers={"X-RateLimit-Remaining": "0", "X-RateLimit-Reset": "1640000000"},
            json={"message": "API rate limit exceeded"},
        )
    )

    adapter = GitHubAdapter("ghp_token")

    try:
        with pytest.raises(ValueError, match="rate limit exceeded.*Resets at"):
            await adapter.get_default_branch("owner", "repo")
    finally:
        await adapter.close()
