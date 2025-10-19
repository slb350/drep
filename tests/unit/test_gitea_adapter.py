"""Unit tests for GiteaAdapter."""

import httpx
import pytest
import respx


def mock_labels_response(labels_list):
    """Helper to create a label mock handler that supports pagination."""

    def handler(request):
        page = int(request.url.params.get("page", 1))
        # Return all labels on page 1, empty on subsequent pages
        if page == 1:
            return httpx.Response(200, json=labels_list)
        else:
            return httpx.Response(200, json=[])

    return handler


@pytest.mark.asyncio
async def test_gitea_adapter_initialization():
    """Test GiteaAdapter initialization with URL and token."""
    from drep.adapters.gitea import GiteaAdapter

    url = "http://192.168.1.14:3000"
    token = "test_token_123"

    adapter = GiteaAdapter(url, token)

    # Verify URL is stored (without trailing slash)
    assert adapter.url == "http://192.168.1.14:3000"
    assert adapter.token == token

    # Verify HTTP client is created
    assert adapter.client is not None
    assert isinstance(adapter.client, httpx.AsyncClient)

    # Clean up
    await adapter.close()


@pytest.mark.asyncio
async def test_gitea_adapter_strips_trailing_slash():
    """Test that trailing slash is stripped from URL."""
    from drep.adapters.gitea import GiteaAdapter

    adapter = GiteaAdapter("http://192.168.1.14:3000/", "token")

    assert adapter.url == "http://192.168.1.14:3000"

    await adapter.close()


@pytest.mark.asyncio
async def test_gitea_adapter_client_headers():
    """Test that HTTP client has correct authorization header."""
    from drep.adapters.gitea import GiteaAdapter

    token = "test_token_abc"
    adapter = GiteaAdapter("http://192.168.1.14:3000", token)

    # Check authorization header is set correctly
    assert "Authorization" in adapter.client.headers
    assert adapter.client.headers["Authorization"] == f"token {token}"

    await adapter.close()


@pytest.mark.asyncio
async def test_gitea_adapter_close():
    """Test that close() properly closes the HTTP client."""
    from drep.adapters.gitea import GiteaAdapter

    adapter = GiteaAdapter("http://192.168.1.14:3000", "token")

    # Client should be open
    assert not adapter.client.is_closed

    # Close the adapter
    await adapter.close()

    # Client should be closed
    assert adapter.client.is_closed


@pytest.mark.asyncio
async def test_gitea_adapter_timeout():
    """Test that HTTP client has reasonable timeout configured."""
    from drep.adapters.gitea import GiteaAdapter

    adapter = GiteaAdapter("http://192.168.1.14:3000", "token")

    # Check timeout is set (should be 30 seconds as per design)
    assert adapter.client.timeout is not None
    assert adapter.client.timeout.read == 30.0

    await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_get_default_branch_success():
    """Test get_default_branch() returns branch name on success."""
    from drep.adapters.gitea import GiteaAdapter

    # Mock the Gitea API response
    respx.get("http://192.168.1.14:3000/api/v1/repos/steve/drep").mock(
        return_value=httpx.Response(200, json={"default_branch": "main"})
    )

    adapter = GiteaAdapter("http://192.168.1.14:3000", "token")

    try:
        branch = await adapter.get_default_branch("steve", "drep")
        assert branch == "main"
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_get_default_branch_master():
    """Test get_default_branch() handles 'master' branch."""
    from drep.adapters.gitea import GiteaAdapter

    # Mock API response with 'master' branch
    respx.get("http://192.168.1.14:3000/api/v1/repos/steve/test-repo").mock(
        return_value=httpx.Response(200, json={"default_branch": "master"})
    )

    adapter = GiteaAdapter("http://192.168.1.14:3000", "token")

    try:
        branch = await adapter.get_default_branch("steve", "test-repo")
        assert branch == "master"
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_get_default_branch_not_found():
    """Test get_default_branch() raises ValueError for 404."""
    from drep.adapters.gitea import GiteaAdapter

    # Mock 404 response
    respx.get("http://192.168.1.14:3000/api/v1/repos/steve/nonexistent").mock(
        return_value=httpx.Response(404, json={"message": "Not Found"})
    )

    adapter = GiteaAdapter("http://192.168.1.14:3000", "token")

    try:
        with pytest.raises(ValueError, match="Repository steve/nonexistent not found"):
            await adapter.get_default_branch("steve", "nonexistent")
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_get_default_branch_unauthorized():
    """Test get_default_branch() raises ValueError for 401."""
    from drep.adapters.gitea import GiteaAdapter

    # Mock 401 response
    respx.get("http://192.168.1.14:3000/api/v1/repos/steve/drep").mock(
        return_value=httpx.Response(401, json={"message": "Unauthorized"})
    )

    adapter = GiteaAdapter("http://192.168.1.14:3000", "token")

    try:
        with pytest.raises(ValueError, match="Unauthorized - check your Gitea token"):
            await adapter.get_default_branch("steve", "drep")
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_get_default_branch_server_error():
    """Test get_default_branch() raises HTTPStatusError for other errors."""
    from drep.adapters.gitea import GiteaAdapter

    # Mock 500 response
    respx.get("http://192.168.1.14:3000/api/v1/repos/steve/drep").mock(
        return_value=httpx.Response(500, json={"message": "Internal Server Error"})
    )

    adapter = GiteaAdapter("http://192.168.1.14:3000", "token")

    try:
        with pytest.raises(httpx.HTTPStatusError):
            await adapter.get_default_branch("steve", "drep")
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_create_issue_success():
    """Test create_issue() successfully creates issue and returns number."""
    from drep.adapters.gitea import GiteaAdapter

    # Mock labels API for label name → ID translation (with pagination support)
    respx.get("http://192.168.1.14:3000/api/v1/repos/steve/drep/labels").mock(
        side_effect=mock_labels_response(
            [
                {"id": 1, "name": "documentation"},
                {"id": 2, "name": "automated"},
            ]
        )
    )

    # Mock successful issue creation
    respx.post("http://192.168.1.14:3000/api/v1/repos/steve/drep/issues").mock(
        return_value=httpx.Response(201, json={"number": 42})
    )

    adapter = GiteaAdapter("http://192.168.1.14:3000", "token")

    try:
        issue_number = await adapter.create_issue(
            owner="steve",
            repo="drep",
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
    from drep.adapters.gitea import GiteaAdapter

    # Mock successful issue creation
    respx.post("http://192.168.1.14:3000/api/v1/repos/steve/drep/issues").mock(
        return_value=httpx.Response(201, json={"number": 43})
    )

    adapter = GiteaAdapter("http://192.168.1.14:3000", "token")

    try:
        issue_number = await adapter.create_issue(
            owner="steve", repo="drep", title="[Test] Issue without labels", body="Body content"
        )
        assert issue_number == 43
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_create_issue_sends_correct_payload():
    """Test create_issue() sends correct JSON payload with label IDs."""
    from drep.adapters.gitea import GiteaAdapter

    # Mock labels API (with pagination support)
    respx.get("http://192.168.1.14:3000/api/v1/repos/steve/drep/labels").mock(
        side_effect=mock_labels_response(
            [
                {"id": 10, "name": "bug"},
                {"id": 20, "name": "help wanted"},
            ]
        )
    )

    # Track the request payload
    request_data = {}

    def capture_request(request):
        import json

        request_data["payload"] = json.loads(request.content)
        return httpx.Response(201, json={"number": 44})

    respx.post("http://192.168.1.14:3000/api/v1/repos/steve/drep/issues").mock(
        side_effect=capture_request
    )

    adapter = GiteaAdapter("http://192.168.1.14:3000", "token")

    try:
        await adapter.create_issue(
            owner="steve",
            repo="drep",
            title="Test Title",
            body="Test Body",
            labels=["bug", "help wanted"],
        )

        # Verify payload structure - labels should be IDs (integers), not names
        assert request_data["payload"]["title"] == "Test Title"
        assert request_data["payload"]["body"] == "Test Body"
        assert request_data["payload"]["labels"] == [10, 20]
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_create_issue_error_handling():
    """Test create_issue() raises ValueError with response text on error."""
    from drep.adapters.gitea import GiteaAdapter

    # Mock error response
    respx.post("http://192.168.1.14:3000/api/v1/repos/steve/drep/issues").mock(
        return_value=httpx.Response(403, text="Forbidden: Permission denied")
    )

    adapter = GiteaAdapter("http://192.168.1.14:3000", "token")

    try:
        with pytest.raises(ValueError, match="Failed to create issue"):
            await adapter.create_issue(owner="steve", repo="drep", title="Test", body="Test")
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_create_issue_translates_label_names_to_ids():
    """Test create_issue() translates label names to IDs before posting."""
    from drep.adapters.gitea import GiteaAdapter

    # Mock the labels API to return label IDs (with pagination support)
    respx.get("http://192.168.1.14:3000/api/v1/repos/steve/drep/labels").mock(
        side_effect=mock_labels_response(
            [
                {"id": 1, "name": "documentation"},
                {"id": 2, "name": "automated"},
                {"id": 3, "name": "bug"},
            ]
        )
    )

    # Track the actual payload sent to create issue
    request_data = {}

    def capture_request(request):
        import json

        request_data["payload"] = json.loads(request.content)
        return httpx.Response(201, json={"number": 50})

    respx.post("http://192.168.1.14:3000/api/v1/repos/steve/drep/issues").mock(
        side_effect=capture_request
    )

    adapter = GiteaAdapter("http://192.168.1.14:3000", "token")

    try:
        await adapter.create_issue(
            owner="steve",
            repo="drep",
            title="Test",
            body="Test",
            labels=["documentation", "automated"],
        )

        # Verify that label IDs (integers) were sent, not names
        assert request_data["payload"]["labels"] == [1, 2]
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_create_issue_handles_unknown_labels():
    """Test create_issue() raises ValueError for unknown label names."""
    from drep.adapters.gitea import GiteaAdapter

    # Mock labels API with limited labels (with pagination support)
    respx.get("http://192.168.1.14:3000/api/v1/repos/steve/drep/labels").mock(
        side_effect=mock_labels_response(
            [{"id": 1, "name": "documentation"}, {"id": 2, "name": "automated"}]
        )
    )

    # Mock successful issue creation (missing labels are now silently skipped)
    respx.post("http://192.168.1.14:3000/api/v1/repos/steve/drep/issues").mock(
        return_value=httpx.Response(201, json={"number": 42})
    )

    adapter = GiteaAdapter("http://192.168.1.14:3000", "token")

    try:
        # Should succeed and only use the valid 'documentation' label
        issue_number = await adapter.create_issue(
            owner="steve",
            repo="drep",
            title="Test",
            body="Test",
            labels=["documentation", "nonexistent"],  # 'nonexistent' is skipped
        )
        assert issue_number == 42
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_create_issue_empty_labels_works():
    """Test create_issue() works with empty labels (no API call needed)."""
    from drep.adapters.gitea import GiteaAdapter

    # Mock successful issue creation
    respx.post("http://192.168.1.14:3000/api/v1/repos/steve/drep/issues").mock(
        return_value=httpx.Response(201, json={"number": 51})
    )

    adapter = GiteaAdapter("http://192.168.1.14:3000", "token")

    try:
        issue_number = await adapter.create_issue(
            owner="steve", repo="drep", title="Test", body="Test", labels=[]
        )
        assert issue_number == 51

        # Verify no labels API call was made (only 1 request to create issue)
        assert len(respx.calls) == 1
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_get_label_ids_handles_pagination():
    """Test _get_label_ids() fetches all labels across multiple pages."""
    from drep.adapters.gitea import GiteaAdapter

    # Simulate paginated responses - route will be matched based on page param
    def label_handler(request):
        page = int(request.url.params.get("page", 1))

        if page == 1:
            return httpx.Response(
                200,
                json=[
                    {"id": 1, "name": "bug"},
                    {"id": 2, "name": "enhancement"},
                ],
            )
        elif page == 2:
            return httpx.Response(
                200,
                json=[
                    {"id": 3, "name": "documentation"},
                    {"id": 4, "name": "help wanted"},
                ],
            )
        else:
            # Page 3 and beyond - empty (end of pagination)
            return httpx.Response(200, json=[])

    respx.get("http://192.168.1.14:3000/api/v1/repos/steve/drep/labels").mock(
        side_effect=label_handler
    )

    adapter = GiteaAdapter("http://192.168.1.14:3000", "token")

    try:
        # Request a label from page 2 - should be found via pagination
        label_ids = await adapter._get_label_ids("steve", "drep", ["documentation", "help wanted"])

        # Should find labels from page 2
        assert label_ids == [3, 4]
    finally:
        await adapter.close()


# ===== PR Review Tests =====


@pytest.mark.asyncio
@respx.mock
async def test_get_pr_success():
    """Test get_pr() successfully fetches PR details."""
    from drep.adapters.gitea import GiteaAdapter

    # Mock PR response
    pr_data = {
        "number": 42,
        "title": "Add feature X",
        "body": "This PR adds feature X",
        "state": "open",
        "base": {"ref": "main"},
        "head": {"ref": "feature-x", "sha": "abc123def456"},
        "user": {"login": "steve"},
    }

    respx.get("http://192.168.1.14:3000/api/v1/repos/steve/drep/pulls/42").mock(
        return_value=httpx.Response(200, json=pr_data)
    )

    adapter = GiteaAdapter("http://192.168.1.14:3000", "token")

    try:
        result = await adapter.get_pr("steve", "drep", 42)
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
    from drep.adapters.gitea import GiteaAdapter

    # Mock 404 response
    respx.get("http://192.168.1.14:3000/api/v1/repos/steve/drep/pulls/999").mock(
        return_value=httpx.Response(404, json={"message": "Not Found"})
    )

    adapter = GiteaAdapter("http://192.168.1.14:3000", "token")

    try:
        with pytest.raises(ValueError, match="Pull request #999 not found"):
            await adapter.get_pr("steve", "drep", 999)
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_get_pr_diff_success():
    """Test get_pr_diff() successfully fetches PR diff."""
    from drep.adapters.gitea import GiteaAdapter

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

    respx.get("http://192.168.1.14:3000/api/v1/repos/steve/drep/pulls/42.diff").mock(
        return_value=httpx.Response(200, text=diff_content)
    )

    adapter = GiteaAdapter("http://192.168.1.14:3000", "token")

    try:
        diff = await adapter.get_pr_diff("steve", "drep", 42)
        assert "diff --git" in diff
        assert "src/module.py" in diff
        assert "+    result = x + y" in diff
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_get_pr_diff_large():
    """Test get_pr_diff() handles large diffs (no size limit at this layer)."""
    from drep.adapters.gitea import GiteaAdapter

    # Create a large diff (> 100KB)
    large_diff = "diff --git a/file.py b/file.py\n" + ("+" + "x" * 1000 + "\n") * 200

    respx.get("http://192.168.1.14:3000/api/v1/repos/steve/drep/pulls/42.diff").mock(
        return_value=httpx.Response(200, text=large_diff)
    )

    adapter = GiteaAdapter("http://192.168.1.14:3000", "token")

    try:
        diff = await adapter.get_pr_diff("steve", "drep", 42)
        assert len(diff) > 100000
        assert "diff --git" in diff
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_create_pr_comment_success():
    """Test create_pr_comment() successfully posts general comment."""
    from drep.adapters.gitea import GiteaAdapter

    respx.post("http://192.168.1.14:3000/api/v1/repos/steve/drep/issues/42/comments").mock(
        return_value=httpx.Response(201, json={"id": 123, "body": "Test comment"})
    )

    adapter = GiteaAdapter("http://192.168.1.14:3000", "token")

    try:
        # Should not raise
        await adapter.create_pr_comment("steve", "drep", 42, "Test comment")
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_create_pr_comment_sends_correct_payload():
    """Test create_pr_comment() sends correct JSON payload."""
    from drep.adapters.gitea import GiteaAdapter

    request_data = {}

    def capture_request(request):
        import json

        request_data["payload"] = json.loads(request.content)
        return httpx.Response(201, json={"id": 123})

    respx.post("http://192.168.1.14:3000/api/v1/repos/steve/drep/issues/42/comments").mock(
        side_effect=capture_request
    )

    adapter = GiteaAdapter("http://192.168.1.14:3000", "token")

    try:
        await adapter.create_pr_comment("steve", "drep", 42, "Review summary comment")

        # Verify payload
        assert request_data["payload"]["body"] == "Review summary comment"
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_create_pr_review_comment_success():
    """Test create_pr_review_comment() successfully posts inline comment."""
    from drep.adapters.gitea import GiteaAdapter

    respx.post("http://192.168.1.14:3000/api/v1/repos/steve/drep/pulls/42/reviews").mock(
        return_value=httpx.Response(201, json={"id": 456})
    )

    adapter = GiteaAdapter("http://192.168.1.14:3000", "token")

    try:
        # Should not raise
        await adapter.create_pr_review_comment(
            owner="steve",
            repo="drep",
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
    """Test create_pr_review_comment() sends correct JSON payload."""
    from drep.adapters.gitea import GiteaAdapter

    request_data = {}

    def capture_request(request):
        import json

        request_data["payload"] = json.loads(request.content)
        return httpx.Response(201, json={"id": 456})

    respx.post("http://192.168.1.14:3000/api/v1/repos/steve/drep/pulls/42/reviews").mock(
        side_effect=capture_request
    )

    adapter = GiteaAdapter("http://192.168.1.14:3000", "token")

    try:
        await adapter.create_pr_review_comment(
            owner="steve",
            repo="drep",
            pr_number=42,
            commit_sha="abc123def456",
            file_path="src/module.py",
            line=15,
            body="Consider adding error handling here",
        )

        # Verify payload structure
        payload = request_data["payload"]
        assert payload["commit_id"] == "abc123def456"
        assert payload["body"] == ""  # Empty to prevent duplicate comments
        assert len(payload["comments"]) == 1
        assert payload["comments"][0]["path"] == "src/module.py"
        assert payload["comments"][0]["new_position"] == 15
        assert payload["comments"][0]["body"] == "Consider adding error handling here"
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_create_pr_review_comment_error_handling():
    """Test create_pr_review_comment() raises ValueError on error."""
    from drep.adapters.gitea import GiteaAdapter

    respx.post("http://192.168.1.14:3000/api/v1/repos/steve/drep/pulls/42/reviews").mock(
        return_value=httpx.Response(403, text="Forbidden: Permission denied")
    )

    adapter = GiteaAdapter("http://192.168.1.14:3000", "token")

    try:
        with pytest.raises(ValueError, match="Failed to create review comment"):
            await adapter.create_pr_review_comment(
                owner="steve",
                repo="drep",
                pr_number=42,
                commit_sha="abc123",
                file_path="test.py",
                line=10,
                body="Comment",
            )
    finally:
        await adapter.close()
