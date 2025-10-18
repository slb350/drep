"""Unit tests for GiteaAdapter."""

import httpx
import pytest
import respx


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

    # Mock labels API for label name → ID translation
    respx.get("http://192.168.1.14:3000/api/v1/repos/steve/drep/labels").mock(
        return_value=httpx.Response(
            200,
            json=[
                {"id": 1, "name": "documentation"},
                {"id": 2, "name": "automated"},
            ],
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

    # Mock labels API
    respx.get("http://192.168.1.14:3000/api/v1/repos/steve/drep/labels").mock(
        return_value=httpx.Response(
            200,
            json=[
                {"id": 10, "name": "bug"},
                {"id": 20, "name": "help wanted"},
            ],
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

    # Mock the labels API to return label IDs
    respx.get("http://192.168.1.14:3000/api/v1/repos/steve/drep/labels").mock(
        return_value=httpx.Response(
            200,
            json=[
                {"id": 1, "name": "documentation"},
                {"id": 2, "name": "automated"},
                {"id": 3, "name": "bug"},
            ],
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

    # Mock labels API with limited labels
    respx.get("http://192.168.1.14:3000/api/v1/repos/steve/drep/labels").mock(
        return_value=httpx.Response(
            200, json=[{"id": 1, "name": "documentation"}, {"id": 2, "name": "automated"}]
        )
    )

    adapter = GiteaAdapter("http://192.168.1.14:3000", "token")

    try:
        with pytest.raises(ValueError, match="Label 'nonexistent' not found"):
            await adapter.create_issue(
                owner="steve",
                repo="drep",
                title="Test",
                body="Test",
                labels=["documentation", "nonexistent"],
            )
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
