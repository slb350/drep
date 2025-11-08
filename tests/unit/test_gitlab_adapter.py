"""Unit tests for GitLabAdapter."""

import base64

import httpx
import pytest
import respx


@pytest.mark.asyncio
async def test_gitlab_adapter_inherits_from_base_adapter():
    """Test that GitLabAdapter inherits from BaseAdapter."""
    from drep.adapters.base import BaseAdapter
    from drep.adapters.gitlab import GitLabAdapter

    assert issubclass(GitLabAdapter, BaseAdapter)


@pytest.mark.asyncio
async def test_gitlab_adapter_initialization_default():
    """Test GitLabAdapter initialization with default GitLab.com."""
    from drep.adapters.gitlab import GitLabAdapter

    token = "glpat_test_token_123"

    adapter = GitLabAdapter(token)

    # Verify URL is set to GitLab.com
    assert adapter.base_url == "https://gitlab.com"
    assert adapter.api_url == "https://gitlab.com/api/v4"
    assert adapter.token == token

    # Verify HTTP client is created
    assert adapter.client is not None
    assert isinstance(adapter.client, httpx.AsyncClient)

    # Clean up
    await adapter.close()


@pytest.mark.asyncio
async def test_gitlab_adapter_initialization_self_hosted():
    """Test GitLabAdapter initialization with self-hosted GitLab."""
    from drep.adapters.gitlab import GitLabAdapter

    token = "glpat_test_token_abc"
    url = "https://gitlab.example.com"

    adapter = GitLabAdapter(token, url)

    # Verify URL is set to custom instance
    assert adapter.base_url == "https://gitlab.example.com"
    assert adapter.api_url == "https://gitlab.example.com/api/v4"
    assert adapter.token == token

    await adapter.close()


@pytest.mark.asyncio
async def test_gitlab_adapter_client_headers():
    """Test that HTTP client has correct PRIVATE-TOKEN header."""
    from drep.adapters.gitlab import GitLabAdapter

    token = "glpat_test_token_xyz"
    adapter = GitLabAdapter(token)

    # Check PRIVATE-TOKEN header is set correctly (GitLab uses PRIVATE-TOKEN, not Bearer!)
    assert "PRIVATE-TOKEN" in adapter.client.headers
    assert adapter.client.headers["PRIVATE-TOKEN"] == token

    # Check Accept header
    assert "Accept" in adapter.client.headers
    assert adapter.client.headers["Accept"] == "application/json"

    await adapter.close()


@pytest.mark.asyncio
async def test_gitlab_adapter_close():
    """Test that close() properly closes the HTTP client."""
    from drep.adapters.gitlab import GitLabAdapter

    adapter = GitLabAdapter("glpat_token")

    # Client should be open
    assert not adapter.client.is_closed

    # Close the adapter
    await adapter.close()

    # Client should be closed
    assert adapter.client.is_closed


@pytest.mark.asyncio
async def test_gitlab_adapter_timeout():
    """Test that HTTP client has reasonable timeout configured."""
    from drep.adapters.gitlab import GitLabAdapter

    adapter = GitLabAdapter("glpat_token")

    # Check timeout is set (should be 30 seconds as per design)
    assert adapter.client.timeout is not None
    assert adapter.client.timeout.read == 30.0

    await adapter.close()


@pytest.mark.asyncio
async def test_gitlab_adapter_empty_token_raises_error():
    """Test that empty token raises ValueError."""
    from drep.adapters.gitlab import GitLabAdapter

    with pytest.raises(ValueError, match="GitLab token cannot be empty"):
        GitLabAdapter("")


@pytest.mark.asyncio
async def test_gitlab_adapter_whitespace_token_raises_error():
    """Test that whitespace-only token raises ValueError."""
    from drep.adapters.gitlab import GitLabAdapter

    with pytest.raises(ValueError, match="GitLab token cannot be empty"):
        GitLabAdapter("   ")


@pytest.mark.asyncio
async def test_gitlab_adapter_invalid_url_raises_error():
    """Test that invalid URL raises ValueError."""
    from drep.adapters.gitlab import GitLabAdapter

    with pytest.raises(ValueError, match="GitLab URL must start with"):
        GitLabAdapter("glpat_token", "ftp://invalid.com")


# ===== _encode_project_path() Tests =====


@pytest.mark.asyncio
async def test_encode_project_path():
    """Test project path URL encoding."""
    from drep.adapters.gitlab import GitLabAdapter

    adapter = GitLabAdapter("glpat_token")

    # owner/repo → owner%2Frepo
    encoded = adapter._encode_project_path("owner", "repo")
    assert encoded == "owner%2Frepo"

    # Test with special characters
    encoded = adapter._encode_project_path("my-org", "my-project")
    assert encoded == "my-org%2Fmy-project"

    # Test with dots and underscores
    encoded = adapter._encode_project_path("org.name", "project_name")
    assert encoded == "org.name%2Fproject_name"

    await adapter.close()


# ===== get_default_branch() Tests =====


@pytest.mark.asyncio
@respx.mock
async def test_get_default_branch_success():
    """Test get_default_branch() successfully retrieves default branch."""
    from drep.adapters.gitlab import GitLabAdapter

    # Mock successful project retrieval
    respx.get("https://gitlab.com/api/v4/projects/owner%2Frepo").mock(
        return_value=httpx.Response(200, json={"default_branch": "main"})
    )

    adapter = GitLabAdapter("glpat_token")

    try:
        branch = await adapter.get_default_branch("owner", "repo")
        assert branch == "main"
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_get_default_branch_404_error():
    """Test get_default_branch() raises ValueError for 404 (project not found)."""
    from drep.adapters.gitlab import GitLabAdapter

    # Mock 404 response
    respx.get("https://gitlab.com/api/v4/projects/owner%2Frepo").mock(
        return_value=httpx.Response(404, text="Project not found")
    )

    adapter = GitLabAdapter("glpat_token")

    try:
        with pytest.raises(ValueError, match="GitLab project owner/repo not found"):
            await adapter.get_default_branch("owner", "repo")
    finally:
        await adapter.close()


# ===== create_issue() Tests =====


@pytest.mark.asyncio
@respx.mock
async def test_create_issue_success():
    """Test create_issue() successfully creates issue and returns IID."""
    from drep.adapters.gitlab import GitLabAdapter

    # Mock successful issue creation (GitLab returns IID, not global ID)
    respx.post("https://gitlab.com/api/v4/projects/owner%2Frepo/issues").mock(
        return_value=httpx.Response(201, json={"iid": 42})
    )

    adapter = GitLabAdapter("glpat_token")

    try:
        issue_iid = await adapter.create_issue(
            owner="owner",
            repo="repo",
            title="[Test] Issue title",
            body="Issue body content",
            labels=["documentation", "automated"],
        )
        assert issue_iid == 42
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_create_issue_without_labels():
    """Test create_issue() works without labels."""
    from drep.adapters.gitlab import GitLabAdapter

    # Mock successful issue creation
    respx.post("https://gitlab.com/api/v4/projects/owner%2Frepo/issues").mock(
        return_value=httpx.Response(201, json={"iid": 43})
    )

    adapter = GitLabAdapter("glpat_token")

    try:
        issue_iid = await adapter.create_issue(
            owner="owner", repo="repo", title="[Test] Issue without labels", body="Body content"
        )
        assert issue_iid == 43
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_create_issue_sends_correct_payload():
    """Test create_issue() sends correct JSON payload with comma-separated labels."""
    from drep.adapters.gitlab import GitLabAdapter

    # Track the request payload
    request_data = {}

    def capture_request(request):
        import json

        request_data["payload"] = json.loads(request.content)
        return httpx.Response(201, json={"iid": 44})

    respx.post("https://gitlab.com/api/v4/projects/owner%2Frepo/issues").mock(
        side_effect=capture_request
    )

    adapter = GitLabAdapter("glpat_token")

    try:
        await adapter.create_issue(
            owner="owner",
            repo="repo",
            title="Test Title",
            body="Test Body",
            labels=["bug", "help wanted"],
        )

        # Verify payload structure - GitLab uses 'description' not 'body'!
        assert request_data["payload"]["title"] == "Test Title"
        assert request_data["payload"]["description"] == "Test Body"
        # GitLab labels are comma-separated string, not array!
        assert request_data["payload"]["labels"] == "bug,help wanted"
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_create_issue_error_handling():
    """Test create_issue() raises ValueError with response text on error."""
    from drep.adapters.gitlab import GitLabAdapter

    # Mock error response
    respx.post("https://gitlab.com/api/v4/projects/owner%2Frepo/issues").mock(
        return_value=httpx.Response(403, text="Forbidden: Permission denied")
    )

    adapter = GitLabAdapter("glpat_token")

    try:
        with pytest.raises(ValueError, match="Failed to create issue"):
            await adapter.create_issue(owner="owner", repo="repo", title="Test", body="Test")
    finally:
        await adapter.close()


# ===== get_pr() Tests =====


@pytest.mark.asyncio
@respx.mock
async def test_get_pr_success():
    """Test get_pr() successfully retrieves merge request details."""
    from drep.adapters.gitlab import GitLabAdapter

    mr_data = {
        "iid": 42,
        "title": "Test MR",
        "description": "Test description",
        "state": "opened",
        "source_branch": "feature",
        "target_branch": "main",
        "author": {"username": "testuser"},
        "diff_refs": {
            "base_sha": "abc123",
            "head_sha": "def456",
            "start_sha": "abc123",
        },
    }

    # Mock successful MR retrieval
    respx.get("https://gitlab.com/api/v4/projects/owner%2Frepo/merge_requests/42").mock(
        return_value=httpx.Response(200, json=mr_data)
    )

    adapter = GitLabAdapter("glpat_token")

    try:
        result = await adapter.get_pr("owner", "repo", 42)
        assert result["iid"] == 42
        assert result["title"] == "Test MR"
        assert "diff_refs" in result
        assert result["diff_refs"]["base_sha"] == "abc123"
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_get_pr_404_error():
    """Test get_pr() raises ValueError for 404 (MR not found)."""
    from drep.adapters.gitlab import GitLabAdapter

    # Mock 404 response
    respx.get("https://gitlab.com/api/v4/projects/owner%2Frepo/merge_requests/99").mock(
        return_value=httpx.Response(404, text="MR not found")
    )

    adapter = GitLabAdapter("glpat_token")

    try:
        with pytest.raises(ValueError, match="Merge request !99 not found"):
            await adapter.get_pr("owner", "repo", 99)
    finally:
        await adapter.close()


# ===== get_pr_diff() Tests =====


@pytest.mark.asyncio
@respx.mock
async def test_get_pr_diff_success():
    """Test get_pr_diff() successfully retrieves and reconstructs diff."""
    from drep.adapters.gitlab import GitLabAdapter

    diffs = [
        {
            "old_path": "file1.py",
            "new_path": "file1.py",
            "diff": "@@ -1,3 +1,4 @@\n import os\n+import sys\n",
        },
        {
            "old_path": "file2.py",
            "new_path": "file2.py",
            "diff": "@@ -1,2 +1,3 @@\n def test():\n+    pass\n",
        },
    ]

    # Mock successful diff retrieval
    respx.get("https://gitlab.com/api/v4/projects/owner%2Frepo/merge_requests/42/diffs").mock(
        return_value=httpx.Response(200, json=diffs)
    )

    adapter = GitLabAdapter("glpat_token")

    try:
        diff = await adapter.get_pr_diff("owner", "repo", 42)

        # Verify diff reconstruction
        assert "diff --git a/file1.py b/file1.py" in diff
        assert "import sys" in diff
        assert "diff --git a/file2.py b/file2.py" in diff
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_get_pr_diff_empty():
    """Test get_pr_diff() handles empty diff array."""
    from drep.adapters.gitlab import GitLabAdapter

    # Mock empty diff
    respx.get("https://gitlab.com/api/v4/projects/owner%2Frepo/merge_requests/42/diffs").mock(
        return_value=httpx.Response(200, json=[])
    )

    adapter = GitLabAdapter("glpat_token")

    try:
        diff = await adapter.get_pr_diff("owner", "repo", 42)
        assert diff == ""
    finally:
        await adapter.close()


# ===== create_pr_comment() Tests =====


@pytest.mark.asyncio
@respx.mock
async def test_create_pr_comment_success():
    """Test create_pr_comment() successfully posts comment."""
    from drep.adapters.gitlab import GitLabAdapter

    # Mock successful comment creation
    respx.post("https://gitlab.com/api/v4/projects/owner%2Frepo/merge_requests/42/notes").mock(
        return_value=httpx.Response(201, json={"id": 123, "body": "Test comment"})
    )

    adapter = GitLabAdapter("glpat_token")

    try:
        # Should not raise
        await adapter.create_pr_comment("owner", "repo", 42, "Test comment")
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_create_pr_comment_error():
    """Test create_pr_comment() raises ValueError on error."""
    from drep.adapters.gitlab import GitLabAdapter

    # Mock error response
    respx.post("https://gitlab.com/api/v4/projects/owner%2Frepo/merge_requests/42/notes").mock(
        return_value=httpx.Response(500, text="Internal Server Error")
    )

    adapter = GitLabAdapter("glpat_token")

    try:
        with pytest.raises(ValueError, match="Failed to create MR comment"):
            await adapter.create_pr_comment("owner", "repo", 42, "Test")
    finally:
        await adapter.close()


# ===== post_review_comment() Tests =====


@pytest.mark.asyncio
@respx.mock
async def test_post_review_comment_success():
    """Test post_review_comment() successfully posts inline comment."""
    from drep.adapters.gitlab import GitLabAdapter

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
    from drep.adapters.gitlab import GitLabAdapter

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
    from drep.adapters.gitlab import GitLabAdapter

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
        with pytest.raises(ValueError, match="missing required 'diff_refs' field"):
            await adapter.post_review_comment("owner", "repo", 42, "src/main.py", 15, "Comment")
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_post_review_comment_400_error():
    """Test post_review_comment() raises ValueError for 400 (invalid position)."""
    from drep.adapters.gitlab import GitLabAdapter

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
        with pytest.raises(ValueError, match="Invalid position for review comment"):
            await adapter.post_review_comment("owner", "repo", 42, "src/main.py", 99, "Comment")
    finally:
        await adapter.close()


# ===== get_file_content() Tests =====


@pytest.mark.asyncio
@respx.mock
async def test_get_file_content_success():
    """Test get_file_content() successfully retrieves and decodes file."""
    from drep.adapters.gitlab import GitLabAdapter

    # GitLab returns base64-encoded content
    content = "print('Hello, World!')\n"
    content_b64 = base64.b64encode(content.encode("utf-8")).decode("utf-8")

    file_data = {
        "file_path": "hello.py",
        "content": content_b64,
    }

    # Mock file retrieval (note: file path is URL-encoded)
    respx.get(
        "https://gitlab.com/api/v4/projects/owner%2Frepo/repository/files/hello.py",
        params={"ref": "main"},
    ).mock(return_value=httpx.Response(200, json=file_data))

    adapter = GitLabAdapter("glpat_token")

    try:
        result = await adapter.get_file_content("owner", "repo", "hello.py", "main")
        assert result == content
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_get_file_content_with_special_chars():
    """Test get_file_content() URL-encodes file path with special characters."""
    from drep.adapters.gitlab import GitLabAdapter

    content = "test content"
    content_b64 = base64.b64encode(content.encode("utf-8")).decode("utf-8")

    file_data = {"content": content_b64}

    # File path with special characters: src/my file.py → src%2Fmy%20file.py
    respx.get(
        "https://gitlab.com/api/v4/projects/owner%2Frepo/repository/files/src%2Fmy%20file.py",
        params={"ref": "main"},
    ).mock(return_value=httpx.Response(200, json=file_data))

    adapter = GitLabAdapter("glpat_token")

    try:
        result = await adapter.get_file_content("owner", "repo", "src/my file.py", "main")
        assert result == content
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_get_file_content_empty_file():
    """Test get_file_content() handles empty files."""
    from drep.adapters.gitlab import GitLabAdapter

    file_data = {"content": ""}

    respx.get(
        "https://gitlab.com/api/v4/projects/owner%2Frepo/repository/files/empty.txt",
        params={"ref": "main"},
    ).mock(return_value=httpx.Response(200, json=file_data))

    adapter = GitLabAdapter("glpat_token")

    try:
        result = await adapter.get_file_content("owner", "repo", "empty.txt", "main")
        assert result == ""
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_get_file_content_404_error():
    """Test get_file_content() raises ValueError for 404 (file not found)."""
    from drep.adapters.gitlab import GitLabAdapter

    respx.get(
        "https://gitlab.com/api/v4/projects/owner%2Frepo/repository/files/missing.py",
        params={"ref": "main"},
    ).mock(return_value=httpx.Response(404, text="File not found"))

    adapter = GitLabAdapter("glpat_token")

    try:
        with pytest.raises(ValueError, match="File missing.py not found"):
            await adapter.get_file_content("owner", "repo", "missing.py", "main")
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_get_file_content_binary_error():
    """Test get_file_content() raises ValueError for binary/non-UTF8 files."""
    from drep.adapters.gitlab import GitLabAdapter

    # Binary content (not valid UTF-8)
    binary_content = b"\x89\x50\x4e\x47"  # PNG header
    content_b64 = base64.b64encode(binary_content).decode("utf-8")

    file_data = {"content": content_b64}

    respx.get(
        "https://gitlab.com/api/v4/projects/owner%2Frepo/repository/files/image.png",
        params={"ref": "main"},
    ).mock(return_value=httpx.Response(200, json=file_data))

    adapter = GitLabAdapter("glpat_token")

    try:
        with pytest.raises(ValueError, match="binary or non-UTF8"):
            await adapter.get_file_content("owner", "repo", "image.png", "main")
    finally:
        await adapter.close()


# ===== Rate Limit Tests =====


@pytest.mark.asyncio
@respx.mock
async def test_rate_limit_detection():
    """Test that rate limit errors are detected and reported."""
    from drep.adapters.gitlab import GitLabAdapter

    # Mock rate limit error (429)
    respx.get("https://gitlab.com/api/v4/projects/owner%2Frepo").mock(
        return_value=httpx.Response(
            429,
            headers={
                "RateLimit-Remaining": "0",
                "RateLimit-Reset": "1234567890",
            },
            text="Rate limit exceeded",
        )
    )

    adapter = GitLabAdapter("glpat_token")

    try:
        with pytest.raises(ValueError, match="GitLab API rate limit exceeded"):
            await adapter.get_default_branch("owner", "repo")
    finally:
        await adapter.close()


# ===== Timeout Tests =====


@pytest.mark.asyncio
@respx.mock
async def test_timeout_error_handling():
    """Test that timeout errors are handled gracefully."""
    from drep.adapters.gitlab import GitLabAdapter

    # Mock timeout
    async def timeout_handler(request):
        raise httpx.TimeoutException("Request timed out")

    respx.get("https://gitlab.com/api/v4/projects/owner%2Frepo").mock(side_effect=timeout_handler)

    adapter = GitLabAdapter("glpat_token")

    try:
        with pytest.raises(ValueError, match="GitLab API request timed out"):
            await adapter.get_default_branch("owner", "repo")
    finally:
        await adapter.close()


# ===== Connection Error Tests =====


@pytest.mark.asyncio
@respx.mock
async def test_connection_error_handling():
    """Test that connection errors are handled gracefully."""
    from drep.adapters.gitlab import GitLabAdapter

    # Mock connection error
    async def connection_error_handler(request):
        raise httpx.ConnectError("Cannot connect to GitLab")

    respx.get("https://gitlab.com/api/v4/projects/owner%2Frepo").mock(
        side_effect=connection_error_handler
    )

    adapter = GitLabAdapter("glpat_token")

    try:
        with pytest.raises(ValueError, match="Cannot connect to GitLab API"):
            await adapter.get_default_branch("owner", "repo")
    finally:
        await adapter.close()


# ===== Self-hosted GitLab Tests =====


@pytest.mark.asyncio
@respx.mock
async def test_self_hosted_gitlab_url():
    """Test adapter works with self-hosted GitLab instances."""
    from drep.adapters.gitlab import GitLabAdapter

    adapter = GitLabAdapter("glpat_token", "https://gitlab.company.com")

    # Mock successful project retrieval from custom instance
    respx.get("https://gitlab.company.com/api/v4/projects/owner%2Frepo").mock(
        return_value=httpx.Response(200, json={"default_branch": "develop"})
    )

    try:
        branch = await adapter.get_default_branch("owner", "repo")
        assert branch == "develop"
    finally:
        await adapter.close()
