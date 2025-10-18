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

    async def get_default_branch(self, owner: str, repo: str) -> str:
        """Get repository default branch.

        Args:
            owner: Repository owner
            repo: Repository name

        Returns:
            Default branch name (e.g., 'main', 'master')

        Raises:
            ValueError: If repository not found (404) or unauthorized (401)
            httpx.HTTPStatusError: For other HTTP errors
        """
        url = f"{self.url}/api/v1/repos/{owner}/{repo}"

        try:
            response = await self.client.get(url)
            response.raise_for_status()
            data = response.json()
            return data["default_branch"]
        except httpx.HTTPStatusError as e:
            if e.response.status_code == 404:
                raise ValueError(f"Repository {owner}/{repo} not found")
            elif e.response.status_code == 401:
                raise ValueError("Unauthorized - check your Gitea token")
            else:
                raise
