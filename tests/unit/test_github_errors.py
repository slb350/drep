"""GitHubAdapter error handling: HTTP codes, network errors, decode and JSON parse failures."""

import base64

import httpx
import pytest
import respx

from drep.adapters.github import GitHubAdapter

# ===== HTTP Error Code Tests =====


@pytest.mark.asyncio
@respx.mock
async def test_create_issue_unauthorized_401():
    """Test create_issue() handles 401 unauthorized error."""

    respx.post("https://api.github.com/repos/owner/repo/issues").mock(
        return_value=httpx.Response(401, json={"message": "Bad credentials"})
    )

    adapter = GitHubAdapter("ghp_token")

    try:
        with pytest.raises(ValueError, match=r"Failed to create issue"):
            await adapter.create_issue("owner", "repo", "Test", "Test")
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_post_review_comment_validation_failed_422():
    """Test post_review_comment() handles 422 validation error for invalid line."""

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
        with pytest.raises(ValueError, match=r"Invalid line number.*Line must be part of PR diff"):
            await adapter.post_review_comment("owner", "repo", 42, "test.py", 999, "Comment")
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_create_issue_server_error_500():
    """Test create_issue() handles 500 server error."""

    respx.post("https://api.github.com/repos/owner/repo/issues").mock(
        return_value=httpx.Response(500, json={"message": "Internal Server Error"})
    )

    adapter = GitHubAdapter("ghp_token")

    try:
        with pytest.raises(ValueError, match=r"Failed to create issue"):
            await adapter.create_issue("owner", "repo", "Test", "Test")
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_get_pr_rate_limit_403():
    """Test get_pr() detects and reports rate limit errors."""

    respx.get("https://api.github.com/repos/owner/repo/pulls/42").mock(
        return_value=httpx.Response(
            403,
            json={"message": "API rate limit exceeded"},
            headers={"X-RateLimit-Remaining": "0", "X-RateLimit-Reset": "1640000000"},
        )
    )

    adapter = GitHubAdapter("ghp_token")

    try:
        with pytest.raises(ValueError, match=r"GitHub API rate limit exceeded.*Resets at"):
            await adapter.get_pr("owner", "repo", 42)
    finally:
        await adapter.close()


# ===== Constructor Validation Tests =====


def test_github_adapter_empty_token_raises_error():
    """Test that GitHubAdapter raises ValueError for empty token."""

    with pytest.raises(ValueError, match=r"GitHub token cannot be empty"):
        GitHubAdapter("")


def test_github_adapter_whitespace_token_raises_error():
    """Test that GitHubAdapter raises ValueError for whitespace-only token."""

    with pytest.raises(ValueError, match=r"GitHub token cannot be empty"):
        GitHubAdapter("   ")


def test_github_adapter_invalid_url_raises_error():
    """Test that GitHubAdapter raises ValueError for invalid URL."""

    with pytest.raises(ValueError, match=r"GitHub URL must start with http"):
        GitHubAdapter("ghp_token", url="ftp://invalid.com")


# ===== Network Error Tests =====


@pytest.mark.asyncio
@respx.mock
async def test_create_issue_timeout_error():
    """Test create_issue() handles timeout errors gracefully."""

    def timeout_handler(request):
        raise httpx.TimeoutException("Request timeout")

    respx.post("https://api.github.com/repos/owner/repo/issues").mock(side_effect=timeout_handler)

    adapter = GitHubAdapter("ghp_token")

    try:
        with pytest.raises(ValueError, match=r"GitHub API request timed out"):
            await adapter.create_issue("owner", "repo", "Test", "Test")
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_get_pr_connection_error():
    """Test get_pr() handles connection errors gracefully."""

    def connection_error_handler(request):
        raise httpx.ConnectError("Connection failed")

    respx.get("https://api.github.com/repos/owner/repo/pulls/42").mock(
        side_effect=connection_error_handler
    )

    adapter = GitHubAdapter("ghp_token")

    try:
        with pytest.raises(ValueError, match=r"Cannot connect to GitHub API"):
            await adapter.get_pr("owner", "repo", 42)
    finally:
        await adapter.close()


# ===== Base64 Decode Error Tests =====


@pytest.mark.asyncio
@respx.mock
async def test_get_file_content_invalid_base64():
    """Test get_file_content() handles corrupted base64 gracefully."""

    # Invalid base64 content
    respx.get("https://api.github.com/repos/owner/repo/contents/corrupted.py").mock(
        return_value=httpx.Response(200, json={"content": "!@#$%^&*()INVALID"})
    )

    adapter = GitHubAdapter("ghp_token")

    try:
        with pytest.raises(ValueError, match=r"Failed to decode file content.*invalid base64"):
            await adapter.get_file_content("owner", "repo", "corrupted.py", "main")
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_get_file_content_binary_file():
    """Test get_file_content() rejects binary files gracefully."""

    # Binary content (non-UTF8)
    binary_data = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR"  # PNG header
    b64_binary = base64.b64encode(binary_data).decode()

    respx.get("https://api.github.com/repos/owner/repo/contents/image.png").mock(
        return_value=httpx.Response(200, json={"content": b64_binary})
    )

    adapter = GitHubAdapter("ghp_token")

    try:
        with pytest.raises(ValueError, match=r"binary or non-UTF8.*only supports text files"):
            await adapter.get_file_content("owner", "repo", "image.png", "main")
    finally:
        await adapter.close()


# ===== JSON Parsing Error Tests =====


@pytest.mark.asyncio
@respx.mock
async def test_create_issue_invalid_json_response():
    """Test create_issue() handles non-JSON responses gracefully."""

    # Return HTML error page instead of JSON
    respx.post("https://api.github.com/repos/owner/repo/issues").mock(
        return_value=httpx.Response(200, text="<html><body>Error</body></html>")
    )

    adapter = GitHubAdapter("ghp_token")

    try:
        with pytest.raises(ValueError, match=r"GitHub API returned invalid JSON"):
            await adapter.create_issue("owner", "repo", "Test", "Test")
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_create_issue_missing_number_field():
    """Test create_issue() validates required fields in response."""

    # Response missing 'number' field
    respx.post("https://api.github.com/repos/owner/repo/issues").mock(
        return_value=httpx.Response(201, json={"id": 12345, "title": "Test"})
    )

    adapter = GitHubAdapter("ghp_token")

    try:
        with pytest.raises(ValueError, match=r"missing 'number' field"):
            await adapter.create_issue("owner", "repo", "Test", "Test")
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_post_review_comment_missing_head_sha():
    """Test post_review_comment() validates nested PR structure."""

    # PR response missing head.sha
    pr_data = {"number": 42, "head": {}}  # Missing 'sha' field
    respx.get("https://api.github.com/repos/owner/repo/pulls/42").mock(
        return_value=httpx.Response(200, json=pr_data)
    )

    adapter = GitHubAdapter("ghp_token")

    try:
        with pytest.raises(ValueError, match=r"missing required 'head.sha' field"):
            await adapter.post_review_comment("owner", "repo", 42, "test.py", 10, "Comment")
    finally:
        await adapter.close()


@pytest.mark.asyncio
async def test_close_propagates_keyboard_interrupt():
    """Test that close() propagates KeyboardInterrupt instead of swallowing it."""
    from unittest.mock import AsyncMock

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

    adapter = GitHubAdapter("ghp_token")

    # Mock aclose() to raise CancelledError
    adapter.client.aclose = AsyncMock(side_effect=asyncio.CancelledError())

    # CancelledError should propagate
    with pytest.raises(asyncio.CancelledError):
        await adapter.close()


@pytest.mark.asyncio
async def test_check_rate_limit_with_zero():
    """Test _check_rate_limit() detects rate limit with '0' string."""

    adapter = GitHubAdapter("ghp_token")
    response = httpx.Response(
        403, headers={"X-RateLimit-Remaining": "0", "X-RateLimit-Reset": "1640000000"}
    )

    with pytest.raises(ValueError, match=r"rate limit exceeded.*Resets at"):
        adapter._check_rate_limit(response, "owner", "repo")

    await adapter.close()


@pytest.mark.asyncio
async def test_check_rate_limit_with_whitespace():
    """Test _check_rate_limit() handles whitespace in header value."""

    adapter = GitHubAdapter("ghp_token")
    response = httpx.Response(
        403, headers={"X-RateLimit-Remaining": " 0 ", "X-RateLimit-Reset": "1640000000"}
    )

    with pytest.raises(ValueError, match=r"rate limit exceeded"):
        adapter._check_rate_limit(response, "owner", "repo")

    await adapter.close()


@pytest.mark.asyncio
async def test_check_rate_limit_with_float():
    """Test _check_rate_limit() handles float string like '0.0'."""

    adapter = GitHubAdapter("ghp_token")
    response = httpx.Response(
        403, headers={"X-RateLimit-Remaining": "0.0", "X-RateLimit-Reset": "1640000000"}
    )

    with pytest.raises(ValueError, match=r"rate limit exceeded"):
        adapter._check_rate_limit(response, "owner", "repo")

    await adapter.close()


@pytest.mark.asyncio
async def test_check_rate_limit_non_zero_remaining():
    """Test _check_rate_limit() doesn't raise when requests remain."""

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

    adapter = GitHubAdapter("ghp_token")
    response = httpx.Response(403, headers={})

    # Should not raise - no rate limit header
    adapter._check_rate_limit(response, "owner", "repo")

    await adapter.close()


@pytest.mark.asyncio
async def test_check_rate_limit_invalid_header_value():
    """Test _check_rate_limit() handles invalid (non-numeric) header value."""

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

    # API response missing 'content' field (malformed response)
    respx.get("https://api.github.com/repos/owner/repo/contents/test.py").mock(
        return_value=httpx.Response(
            200,
            json={"name": "test.py", "path": "test.py", "type": "file"},  # Missing 'content'
        )
    )

    adapter = GitHubAdapter("ghp_token")

    try:
        with pytest.raises(ValueError, match=r"missing 'content' field"):
            await adapter.get_file_content("owner", "repo", "test.py", "main")
    finally:
        await adapter.close()
