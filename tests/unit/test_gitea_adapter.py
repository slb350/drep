"""Unit tests for GiteaAdapter."""

import httpx
import pytest


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
