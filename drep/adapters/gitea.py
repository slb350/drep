"""Gitea platform adapter implementation."""


import httpx


class GiteaAdapter:
    """Gitea API adapter."""

    def __init__(self, url: str, token: str):
        """Initialize GiteaAdapter with URL and token.

        Args:
            url: Gitea base URL (e.g., http://192.168.1.14:3000)
            token: Gitea API token for authentication
        """
        self.url = url.rstrip("/")
        self.token = token
        self.client = httpx.AsyncClient(headers={"Authorization": f"token {token}"}, timeout=30.0)

    async def close(self):
        """Close HTTP client connection."""
        await self.client.aclose()
