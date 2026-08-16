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
        with pytest.raises(ValueError, match=r"Failed to create issue"):
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
        with pytest.raises(ValueError, match=r"File nonexistent.py not found"):
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
        with pytest.raises(ValueError, match=r"Repository owner/nonexistent not found"):
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
            200,
            json={"name": "repo", "owner": {"login": "owner"}},  # Missing default_branch
        )
    )

    adapter = GitHubAdapter("ghp_token")

    try:
        with pytest.raises(ValueError, match=r"missing 'default_branch' field"):
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
        with pytest.raises(ValueError, match=r"timed out"):
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
        with pytest.raises(ValueError, match=r"invalid JSON"):
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
        with pytest.raises(ValueError, match=r"Cannot connect to GitHub API"):
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
        with pytest.raises(ValueError, match=r"rate limit exceeded.*Resets at"):
            await adapter.get_default_branch("owner", "repo")
    finally:
        await adapter.close()
