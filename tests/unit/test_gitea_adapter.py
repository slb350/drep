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
