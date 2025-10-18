"""Gitea platform adapter implementation."""

from typing import Dict, List, Optional

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

    async def _get_label_ids(self, owner: str, repo: str, label_names: List[str]) -> List[int]:
        """Get label IDs from label names.

        Args:
            owner: Repository owner
            repo: Repository name
            label_names: List of label names to translate

        Returns:
            List of label IDs corresponding to the label names

        Raises:
            ValueError: If any label name is not found in the repository
        """
        if not label_names:
            return []

        # Fetch all labels from the repository (handle pagination)
        base_url = f"{self.url}/api/v1/repos/{owner}/{repo}/labels"
        all_labels = []
        page = 1

        while True:
            # Fetch current page
            response = await self.client.get(base_url, params={"page": page})
            response.raise_for_status()
            labels = response.json()

            # If page is empty, we've reached the end
            if not labels:
                break

            all_labels.extend(labels)
            page += 1

        # Build name → ID mapping
        label_map: Dict[str, int] = {label["name"]: label["id"] for label in all_labels}

        # Translate names to IDs
        label_ids = []
        for name in label_names:
            if name not in label_map:
                raise ValueError(
                    f"Label '{name}' not found in repository {owner}/{repo}. "
                    f"Available labels: {', '.join(label_map.keys())}"
                )
            label_ids.append(label_map[name])

        return label_ids

    async def create_issue(
        self, owner: str, repo: str, title: str, body: str, labels: Optional[List[str]] = None
    ) -> int:
        """Create an issue and return issue number.

        Args:
            owner: Repository owner
            repo: Repository name
            title: Issue title
            body: Issue body (markdown supported)
            labels: Optional list of label names (will be translated to IDs)

        Returns:
            Created issue number

        Raises:
            ValueError: If issue creation fails or label names are invalid
        """
        # Translate label names to IDs
        label_ids = await self._get_label_ids(owner, repo, labels or [])

        url = f"{self.url}/api/v1/repos/{owner}/{repo}/issues"
        payload = {"title": title, "body": body, "labels": label_ids}

        try:
            response = await self.client.post(url, json=payload)
            response.raise_for_status()
            data = response.json()
            return data["number"]
        except httpx.HTTPStatusError as e:
            raise ValueError(f"Failed to create issue: {e.response.text}")
