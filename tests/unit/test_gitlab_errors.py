"""GitLabAdapter error handling: JSON validation, timeouts, connection errors, HTTP codes."""

import httpx
import pytest
import respx

from drep.adapters.gitlab import GitLabAdapter

# ===== JSON Validation Tests =====


@pytest.mark.asyncio
@respx.mock
async def test_get_default_branch_invalid_json():
    """Test get_default_branch() handles non-JSON responses gracefully."""

    # Return HTML error page instead of JSON
    respx.get("https://gitlab.com/api/v4/projects/owner%2Frepo").mock(
        return_value=httpx.Response(200, text="<html><body>Error</body></html>")
    )

    adapter = GitLabAdapter("glpat_token")

    try:
        with pytest.raises(ValueError, match=r"GitLab API returned invalid JSON"):
            await adapter.get_default_branch("owner", "repo")
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_get_default_branch_missing_field():
    """Test get_default_branch() validates required fields in response."""

    # Response missing 'default_branch' field
    respx.get("https://gitlab.com/api/v4/projects/owner%2Frepo").mock(
        return_value=httpx.Response(200, json={"id": 12345, "name": "repo"})
    )

    adapter = GitLabAdapter("glpat_token")

    try:
        with pytest.raises(ValueError, match=r"missing 'default_branch' field"):
            await adapter.get_default_branch("owner", "repo")
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_create_issue_invalid_json():
    """Test create_issue() handles non-JSON responses gracefully."""

    # Return HTML error page instead of JSON
    respx.post("https://gitlab.com/api/v4/projects/owner%2Frepo/issues").mock(
        return_value=httpx.Response(201, text="<html><body>Created</body></html>")
    )

    adapter = GitLabAdapter("glpat_token")

    try:
        with pytest.raises(ValueError, match=r"GitLab API returned invalid JSON"):
            await adapter.create_issue("owner", "repo", "Test", "Test body")
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_create_issue_missing_iid():
    """Test create_issue() validates required fields in response."""

    # Response missing 'iid' field
    respx.post("https://gitlab.com/api/v4/projects/owner%2Frepo/issues").mock(
        return_value=httpx.Response(201, json={"id": 12345, "title": "Test"})
    )

    adapter = GitLabAdapter("glpat_token")

    try:
        with pytest.raises(ValueError, match=r"missing 'iid' field"):
            await adapter.create_issue("owner", "repo", "Test", "Test body")
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_get_file_content_invalid_json():
    """Test get_file_content() handles non-JSON responses gracefully."""

    # Return HTML error page instead of JSON
    respx.get(
        "https://gitlab.com/api/v4/projects/owner%2Frepo/repository/files/test.py",
        params={"ref": "main"},
    ).mock(return_value=httpx.Response(200, text="<html><body>Error</body></html>"))

    adapter = GitLabAdapter("glpat_token")

    try:
        with pytest.raises(ValueError, match=r"GitLab API returned invalid JSON"):
            await adapter.get_file_content("owner", "repo", "test.py", "main")
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_get_file_content_missing_content_field():
    """Test get_file_content() validates required fields in response."""

    # Response missing 'content' field
    respx.get(
        "https://gitlab.com/api/v4/projects/owner%2Frepo/repository/files/test.py",
        params={"ref": "main"},
    ).mock(return_value=httpx.Response(200, json={"file_path": "test.py", "size": 100}))

    adapter = GitLabAdapter("glpat_token")

    try:
        with pytest.raises(ValueError, match=r"missing 'content' field"):
            await adapter.get_file_content("owner", "repo", "test.py", "main")
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_get_pr_invalid_json():
    """Test get_pr() handles non-JSON responses gracefully."""

    # Return HTML error page instead of JSON
    respx.get("https://gitlab.com/api/v4/projects/owner%2Frepo/merge_requests/42").mock(
        return_value=httpx.Response(200, text="<html><body>Error</body></html>")
    )

    adapter = GitLabAdapter("glpat_token")

    try:
        with pytest.raises(ValueError, match=r"GitLab API returned invalid JSON"):
            await adapter.get_pr("owner", "repo", 42)
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_get_pr_missing_diff_refs():
    """Test get_pr() validates required nested fields in response."""

    # Response missing 'diff_refs' field
    respx.get("https://gitlab.com/api/v4/projects/owner%2Frepo/merge_requests/42").mock(
        return_value=httpx.Response(200, json={"iid": 42, "title": "Test MR"})
    )

    adapter = GitLabAdapter("glpat_token")

    try:
        with pytest.raises(ValueError, match=r"missing 'diff_refs' field"):
            await adapter.get_pr("owner", "repo", 42)
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_get_pr_missing_base_sha():
    """Test get_pr() validates diff_refs.base_sha field."""

    # Response missing 'diff_refs.base_sha' field
    respx.get("https://gitlab.com/api/v4/projects/owner%2Frepo/merge_requests/42").mock(
        return_value=httpx.Response(
            200, json={"iid": 42, "title": "Test MR", "diff_refs": {"head_sha": "def456"}}
        )
    )

    adapter = GitLabAdapter("glpat_token")

    try:
        with pytest.raises(ValueError, match=r"missing 'base_sha'"):
            await adapter.get_pr("owner", "repo", 42)
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_get_pr_missing_head_sha():
    """Test get_pr() validates diff_refs.head_sha field."""

    # Response missing 'diff_refs.head_sha' field
    respx.get("https://gitlab.com/api/v4/projects/owner%2Frepo/merge_requests/42").mock(
        return_value=httpx.Response(
            200, json={"iid": 42, "title": "Test MR", "diff_refs": {"base_sha": "abc123"}}
        )
    )

    adapter = GitLabAdapter("glpat_token")

    try:
        with pytest.raises(ValueError, match=r"missing 'head_sha'"):
            await adapter.get_pr("owner", "repo", 42)
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_get_pr_diff_invalid_json():
    """Test get_pr_diff() handles non-JSON responses gracefully."""

    # Return HTML error page instead of JSON
    respx.get("https://gitlab.com/api/v4/projects/owner%2Frepo/merge_requests/42/diffs").mock(
        return_value=httpx.Response(200, text="<html><body>Error</body></html>")
    )

    adapter = GitLabAdapter("glpat_token")

    try:
        with pytest.raises(ValueError, match=r"GitLab API returned invalid JSON"):
            await adapter.get_pr_diff("owner", "repo", 42)
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_get_pr_diff_not_an_array():
    """Test get_pr_diff() validates response is an array."""

    # Response is an object instead of array (invalid format)
    respx.get("https://gitlab.com/api/v4/projects/owner%2Frepo/merge_requests/42/diffs").mock(
        return_value=httpx.Response(200, json={"error": "Invalid response"})
    )

    adapter = GitLabAdapter("glpat_token")

    try:
        with pytest.raises(ValueError, match=r"expected array"):
            await adapter.get_pr_diff("owner", "repo", 42)
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_post_review_comment_invalid_json():
    """Test post_review_comment() handles non-JSON responses gracefully."""

    # Mock get_pr to return valid MR data
    respx.get("https://gitlab.com/api/v4/projects/owner%2Frepo/merge_requests/42").mock(
        return_value=httpx.Response(
            200,
            json={
                "iid": 42,
                "title": "Test MR",
                "diff_refs": {"base_sha": "abc123", "head_sha": "def456", "start_sha": "abc123"},
            },
        )
    )

    # Mock post to return HTML instead of JSON
    respx.post(
        "https://gitlab.com/api/v4/projects/owner%2Frepo/merge_requests/42/discussions"
    ).mock(return_value=httpx.Response(201, text="<html><body>Created</body></html>"))

    adapter = GitLabAdapter("glpat_token")

    try:
        with pytest.raises(ValueError, match=r"GitLab API returned invalid JSON"):
            await adapter.post_review_comment("owner", "repo", 42, "test.py", 10, "Test comment")
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_create_pr_comment_invalid_json():
    """Test create_pr_comment() handles non-JSON responses gracefully."""

    # Return HTML error page instead of JSON
    respx.post("https://gitlab.com/api/v4/projects/owner%2Frepo/merge_requests/42/notes").mock(
        return_value=httpx.Response(201, text="<html><body>Created</body></html>")
    )

    adapter = GitLabAdapter("glpat_token")

    try:
        with pytest.raises(ValueError, match=r"GitLab API returned invalid JSON"):
            await adapter.create_pr_comment("owner", "repo", 42, "Test comment")
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_post_review_comment_get_pr_fails_validation():
    """Test post_review_comment() when get_pr() fails validation.

    This tests the dependency chain - post_review_comment calls get_pr first.
    """

    # Mock get_pr to return invalid data (missing diff_refs)
    respx.get("https://gitlab.com/api/v4/projects/owner%2Frepo/merge_requests/42").mock(
        return_value=httpx.Response(200, json={"iid": 42, "title": "Test MR"})
    )

    adapter = GitLabAdapter("glpat_token")

    try:
        with pytest.raises(ValueError, match=r"missing 'diff_refs' field"):
            await adapter.post_review_comment("owner", "repo", 42, "test.py", 10, "Test comment")
    finally:
        await adapter.close()


# ===== Timeout Tests =====


@pytest.mark.asyncio
@respx.mock
async def test_timeout_error_handling():
    """Test that timeout errors are handled gracefully."""

    # Mock timeout
    async def timeout_handler(request):
        raise httpx.TimeoutException("Request timed out")

    respx.get("https://gitlab.com/api/v4/projects/owner%2Frepo").mock(side_effect=timeout_handler)

    adapter = GitLabAdapter("glpat_token")

    try:
        with pytest.raises(ValueError, match=r"GitLab API request timed out"):
            await adapter.get_default_branch("owner", "repo")
    finally:
        await adapter.close()


# ===== Connection Error Tests =====


@pytest.mark.asyncio
@respx.mock
async def test_connection_error_handling():
    """Test that connection errors are handled gracefully."""

    # Mock connection error
    async def connection_error_handler(request):
        raise httpx.ConnectError("Cannot connect to GitLab")

    respx.get("https://gitlab.com/api/v4/projects/owner%2Frepo").mock(
        side_effect=connection_error_handler
    )

    adapter = GitLabAdapter("glpat_token")

    try:
        with pytest.raises(ValueError, match=r"Cannot connect to GitLab API"):
            await adapter.get_default_branch("owner", "repo")
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_create_issue_timeout():
    """Test create_issue() handles timeout errors."""

    async def timeout_handler(request):
        raise httpx.TimeoutException("Request timed out")

    respx.post("https://gitlab.com/api/v4/projects/owner%2Frepo/issues").mock(
        side_effect=timeout_handler
    )

    adapter = GitLabAdapter("glpat_token")

    try:
        with pytest.raises(ValueError, match=r"GitLab API request timed out"):
            await adapter.create_issue("owner", "repo", "Title", "Body")
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_create_issue_connection_error():
    """Test create_issue() handles connection errors."""

    async def connection_error_handler(request):
        raise httpx.ConnectError("Cannot connect")

    respx.post("https://gitlab.com/api/v4/projects/owner%2Frepo/issues").mock(
        side_effect=connection_error_handler
    )

    adapter = GitLabAdapter("glpat_token")

    try:
        with pytest.raises(ValueError, match=r"Cannot connect to GitLab API"):
            await adapter.create_issue("owner", "repo", "Title", "Body")
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_get_pr_timeout():
    """Test get_pr() handles timeout errors."""

    async def timeout_handler(request):
        raise httpx.TimeoutException("Request timed out")

    respx.get("https://gitlab.com/api/v4/projects/owner%2Frepo/merge_requests/42").mock(
        side_effect=timeout_handler
    )

    adapter = GitLabAdapter("glpat_token")

    try:
        with pytest.raises(ValueError, match=r"GitLab API request timed out"):
            await adapter.get_pr("owner", "repo", 42)
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_get_pr_connection_error():
    """Test get_pr() handles connection errors."""

    async def connection_error_handler(request):
        raise httpx.ConnectError("Cannot connect")

    respx.get("https://gitlab.com/api/v4/projects/owner%2Frepo/merge_requests/42").mock(
        side_effect=connection_error_handler
    )

    adapter = GitLabAdapter("glpat_token")

    try:
        with pytest.raises(ValueError, match=r"Cannot connect to GitLab API"):
            await adapter.get_pr("owner", "repo", 42)
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_get_pr_diff_timeout():
    """Test get_pr_diff() handles timeout errors."""

    async def timeout_handler(request):
        raise httpx.TimeoutException("Request timed out")

    respx.get("https://gitlab.com/api/v4/projects/owner%2Frepo/merge_requests/42/diffs").mock(
        side_effect=timeout_handler
    )

    adapter = GitLabAdapter("glpat_token")

    try:
        with pytest.raises(ValueError, match=r"GitLab API request timed out"):
            await adapter.get_pr_diff("owner", "repo", 42)
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_get_pr_diff_connection_error():
    """Test get_pr_diff() handles connection errors."""

    async def connection_error_handler(request):
        raise httpx.ConnectError("Cannot connect")

    respx.get("https://gitlab.com/api/v4/projects/owner%2Frepo/merge_requests/42/diffs").mock(
        side_effect=connection_error_handler
    )

    adapter = GitLabAdapter("glpat_token")

    try:
        with pytest.raises(ValueError, match=r"Cannot connect to GitLab API"):
            await adapter.get_pr_diff("owner", "repo", 42)
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_create_pr_comment_timeout():
    """Test create_pr_comment() handles timeout errors."""

    async def timeout_handler(request):
        raise httpx.TimeoutException("Request timed out")

    respx.post("https://gitlab.com/api/v4/projects/owner%2Frepo/merge_requests/42/notes").mock(
        side_effect=timeout_handler
    )

    adapter = GitLabAdapter("glpat_token")

    try:
        with pytest.raises(ValueError, match=r"GitLab API request timed out"):
            await adapter.create_pr_comment("owner", "repo", 42, "Comment")
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_create_pr_comment_connection_error():
    """Test create_pr_comment() handles connection errors."""

    async def connection_error_handler(request):
        raise httpx.ConnectError("Cannot connect")

    respx.post("https://gitlab.com/api/v4/projects/owner%2Frepo/merge_requests/42/notes").mock(
        side_effect=connection_error_handler
    )

    adapter = GitLabAdapter("glpat_token")

    try:
        with pytest.raises(ValueError, match=r"Cannot connect to GitLab API"):
            await adapter.create_pr_comment("owner", "repo", 42, "Comment")
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_post_review_comment_timeout():
    """Test post_review_comment() handles timeout errors."""

    # Mock get_pr to succeed
    respx.get("https://gitlab.com/api/v4/projects/owner%2Frepo/merge_requests/42").mock(
        return_value=httpx.Response(
            200,
            json={
                "iid": 42,
                "diff_refs": {"base_sha": "abc123", "head_sha": "def456", "start_sha": "abc123"},
            },
        )
    )

    # Mock post to timeout
    async def timeout_handler(request):
        raise httpx.TimeoutException("Request timed out")

    respx.post(
        "https://gitlab.com/api/v4/projects/owner%2Frepo/merge_requests/42/discussions"
    ).mock(side_effect=timeout_handler)

    adapter = GitLabAdapter("glpat_token")

    try:
        with pytest.raises(ValueError, match=r"GitLab API request timed out"):
            await adapter.post_review_comment("owner", "repo", 42, "test.py", 10, "Comment")
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_post_review_comment_connection_error():
    """Test post_review_comment() handles connection errors."""

    # Mock get_pr to succeed
    respx.get("https://gitlab.com/api/v4/projects/owner%2Frepo/merge_requests/42").mock(
        return_value=httpx.Response(
            200,
            json={
                "iid": 42,
                "diff_refs": {"base_sha": "abc123", "head_sha": "def456", "start_sha": "abc123"},
            },
        )
    )

    # Mock post to fail connection
    async def connection_error_handler(request):
        raise httpx.ConnectError("Cannot connect")

    respx.post(
        "https://gitlab.com/api/v4/projects/owner%2Frepo/merge_requests/42/discussions"
    ).mock(side_effect=connection_error_handler)

    adapter = GitLabAdapter("glpat_token")

    try:
        with pytest.raises(ValueError, match=r"Cannot connect to GitLab API"):
            await adapter.post_review_comment("owner", "repo", 42, "test.py", 10, "Comment")
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_get_file_content_timeout():
    """Test get_file_content() handles timeout errors."""

    async def timeout_handler(request):
        raise httpx.TimeoutException("Request timed out")

    respx.get(
        "https://gitlab.com/api/v4/projects/owner%2Frepo/repository/files/test.py",
        params={"ref": "main"},
    ).mock(side_effect=timeout_handler)

    adapter = GitLabAdapter("glpat_token")

    try:
        with pytest.raises(ValueError, match=r"GitLab API request timed out"):
            await adapter.get_file_content("owner", "repo", "test.py", "main")
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_get_file_content_connection_error():
    """Test get_file_content() handles connection errors."""

    async def connection_error_handler(request):
        raise httpx.ConnectError("Cannot connect")

    respx.get(
        "https://gitlab.com/api/v4/projects/owner%2Frepo/repository/files/test.py",
        params={"ref": "main"},
    ).mock(side_effect=connection_error_handler)

    adapter = GitLabAdapter("glpat_token")

    try:
        with pytest.raises(ValueError, match=r"Cannot connect to GitLab API"):
            await adapter.get_file_content("owner", "repo", "test.py", "main")
    finally:
        await adapter.close()


# ===== HTTP Error Code Tests =====


@pytest.mark.parametrize(
    "status_code,error_type",
    [
        (401, "Unauthorized"),
        (403, "Forbidden"),
        (500, "Internal Server Error"),
        (503, "Service Unavailable"),
    ],
)
@pytest.mark.asyncio
@respx.mock
async def test_get_default_branch_http_errors(status_code, error_type):
    """Test get_default_branch() handles various HTTP error codes."""

    respx.get("https://gitlab.com/api/v4/projects/owner%2Frepo").mock(
        return_value=httpx.Response(status_code, text=f"{error_type} error")
    )

    adapter = GitLabAdapter("glpat_token")

    try:
        with pytest.raises(ValueError):
            await adapter.get_default_branch("owner", "repo")
    finally:
        await adapter.close()


@pytest.mark.parametrize("status_code", [401, 403, 500, 503])
@pytest.mark.asyncio
@respx.mock
async def test_create_issue_http_errors(status_code):
    """Test create_issue() handles various HTTP error codes."""

    respx.post("https://gitlab.com/api/v4/projects/owner%2Frepo/issues").mock(
        return_value=httpx.Response(status_code, text="Error")
    )

    adapter = GitLabAdapter("glpat_token")

    try:
        with pytest.raises(ValueError):
            await adapter.create_issue("owner", "repo", "Title", "Body")
    finally:
        await adapter.close()


@pytest.mark.parametrize("status_code", [401, 403, 500, 503])
@pytest.mark.asyncio
@respx.mock
async def test_get_pr_http_errors(status_code):
    """Test get_pr() handles various HTTP error codes."""

    respx.get("https://gitlab.com/api/v4/projects/owner%2Frepo/merge_requests/42").mock(
        return_value=httpx.Response(status_code, text="Error")
    )

    adapter = GitLabAdapter("glpat_token")

    try:
        with pytest.raises(ValueError):
            await adapter.get_pr("owner", "repo", 42)
    finally:
        await adapter.close()


@pytest.mark.parametrize("status_code", [401, 403, 500, 503])
@pytest.mark.asyncio
@respx.mock
async def test_get_file_content_http_errors(status_code):
    """Test get_file_content() handles various HTTP error codes."""

    respx.get(
        "https://gitlab.com/api/v4/projects/owner%2Frepo/repository/files/test.py",
        params={"ref": "main"},
    ).mock(return_value=httpx.Response(status_code, text="Error"))

    adapter = GitLabAdapter("glpat_token")

    try:
        with pytest.raises(ValueError):
            await adapter.get_file_content("owner", "repo", "test.py", "main")
    finally:
        await adapter.close()


@pytest.mark.parametrize("status_code", [401, 403, 500, 503])
@pytest.mark.asyncio
@respx.mock
async def test_create_pr_comment_http_errors(status_code):
    """Test create_pr_comment() handles various HTTP error codes."""

    respx.post("https://gitlab.com/api/v4/projects/owner%2Frepo/merge_requests/42/notes").mock(
        return_value=httpx.Response(status_code, text="Error")
    )

    adapter = GitLabAdapter("glpat_token")

    try:
        with pytest.raises(ValueError):
            await adapter.create_pr_comment("owner", "repo", 42, "Comment")
    finally:
        await adapter.close()
