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
        # Cache label maps per repository to avoid redundant API calls
        # Format: {(owner, repo): {label_name: label_id}}
        self._label_cache: Dict[tuple, Dict[str, int]] = {}

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
        """Get label IDs from label names (with caching).

        Args:
            owner: Repository owner
            repo: Repository name
            label_names: List of label names to translate

        Returns:
            List of label IDs for labels that exist (skips missing labels)

        Note:
            Missing labels are silently skipped rather than raising errors.
            This makes the system resilient to missing labels in repositories.
        """
        if not label_names:
            return []

        # Check cache first
        cache_key = (owner, repo)
        if cache_key not in self._label_cache:
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

            # Build name → ID mapping and cache it
            self._label_cache[cache_key] = {label["name"]: label["id"] for label in all_labels}

        # Use cached label map
        label_map = self._label_cache[cache_key]

        # Translate names to IDs (skip missing labels)
        label_ids = []
        for name in label_names:
            if name in label_map:
                label_ids.append(label_map[name])
            # Silently skip missing labels - this is by design for flexibility

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

    # ===== PR Review Methods =====

    async def get_pr(self, owner: str, repo: str, pr_number: int) -> Dict:
        """Get pull request details.

        Args:
            owner: Repository owner
            repo: Repository name
            pr_number: Pull request number

        Returns:
            PR data dictionary with keys: number, title, body, state, base, head, user

        Raises:
            ValueError: If PR not found (404)
            httpx.HTTPStatusError: For other HTTP errors
        """
        url = f"{self.url}/api/v1/repos/{owner}/{repo}/pulls/{pr_number}"

        try:
            response = await self.client.get(url)
            response.raise_for_status()
            return response.json()
        except httpx.HTTPStatusError as e:
            if e.response.status_code == 404:
                raise ValueError(f"Pull request #{pr_number} not found")
            else:
                raise

    async def get_pr_diff(self, owner: str, repo: str, pr_number: int) -> str:
        """Get pull request diff in unified diff format.

        Args:
            owner: Repository owner
            repo: Repository name
            pr_number: Pull request number

        Returns:
            Unified diff string (can be very large)

        Raises:
            httpx.HTTPStatusError: For HTTP errors
        """
        url = f"{self.url}/api/v1/repos/{owner}/{repo}/pulls/{pr_number}.diff"

        response = await self.client.get(url)
        response.raise_for_status()
        return response.text

    async def create_pr_comment(self, owner: str, repo: str, pr_number: int, body: str) -> None:
        """Post a general comment on the PR (not line-specific).

        Args:
            owner: Repository owner
            repo: Repository name
            pr_number: Pull request number
            body: Comment body (markdown supported)

        Raises:
            ValueError: If comment creation fails
        """
        url = f"{self.url}/api/v1/repos/{owner}/{repo}/issues/{pr_number}/comments"
        payload = {"body": body}

        try:
            response = await self.client.post(url, json=payload)
            response.raise_for_status()
        except httpx.HTTPStatusError as e:
            raise ValueError(f"Failed to create PR comment: {e.response.text}")

    async def create_pr_review_comment(
        self,
        owner: str,
        repo: str,
        pr_number: int,
        commit_sha: str,
        file_path: str,
        line: int,
        body: str,
    ) -> None:
        """Post an inline review comment on specific line.

        Args:
            owner: Repository owner
            repo: Repository name
            pr_number: Pull request number
            commit_sha: Commit SHA to comment on (usually PR head)
            file_path: File path relative to repo root
            line: Line number in new version (after changes)
            body: Comment body (markdown supported)

        Raises:
            ValueError: If review comment creation fails
        """
        url = f"{self.url}/api/v1/repos/{owner}/{repo}/pulls/{pr_number}/reviews"

        # Gitea review API format: create a review with inline comments
        # Note: Leave top-level body empty to avoid duplicate comments
        # The body appears in the inline comment only
        # First attempt: use new_position (works on newer Gitea versions)
        payload_new = {
            "commit_id": commit_sha,
            "body": "",
            "comments": [{"path": file_path, "new_position": line, "body": body}],
        }

        try:
            resp = await self.client.post(url, json=payload_new)
            resp.raise_for_status()
            return
        except httpx.HTTPStatusError as e1:
            # Fallback: try "position" for compatibility with other versions
            payload_pos = {
                "commit_id": commit_sha,
                "body": "",
                "comments": [{"path": file_path, "position": line, "body": body}],
            }
            try:
                resp2 = await self.client.post(url, json=payload_pos)
                resp2.raise_for_status()
                return
            except httpx.HTTPStatusError as e2:
                # Propagate detailed error from both attempts
                raise ValueError(
                    f"Failed to create review comment. new_position error: {e1.response.text}; "
                    f"position error: {e2.response.text}"
                )
