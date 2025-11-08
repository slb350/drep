"""GitHub platform adapter implementation."""

import base64
from typing import Dict, List, Optional

import httpx

from drep.adapters.base import BaseAdapter


class GitHubAdapter(BaseAdapter):
    """GitHub API adapter.

    Uses GitHub REST API v3 for all operations. GitHub has different API
    patterns compared to Gitea:
    - Authentication: Bearer token instead of token
    - Labels: Use names directly (not IDs)
    - Review comments: Different endpoint structure
    - Line numbers: Uses line + side instead of position
    """

    def __init__(self, token: str, url: str = "https://api.github.com"):
        """Initialize GitHubAdapter with token.

        Args:
            token: GitHub Personal Access Token (PAT) or GitHub App token
            url: GitHub API base URL (default: https://api.github.com)
                 Can be overridden for GitHub Enterprise Server
        """
        self.url = url.rstrip("/")
        self.token = token
        self.client = httpx.AsyncClient(
            headers={
                "Authorization": f"Bearer {token}",
                "Accept": "application/vnd.github.v3+json",
            },
            timeout=30.0,
        )

    async def close(self):
        """Close HTTP client connection."""
        await self.client.aclose()

    async def create_issue(
        self, owner: str, repo: str, title: str, body: str, labels: Optional[List[str]] = None
    ) -> int:
        """Create an issue and return issue number.

        Args:
            owner: Repository owner
            repo: Repository name
            title: Issue title
            body: Issue body (markdown supported)
            labels: Optional list of label names (GitHub uses names, not IDs)

        Returns:
            Created issue number

        Raises:
            ValueError: If issue creation fails
        """
        url = f"{self.url}/repos/{owner}/{repo}/issues"
        payload = {"title": title, "body": body}

        # GitHub uses label names directly (not IDs like Gitea)
        if labels:
            payload["labels"] = labels

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
        url = f"{self.url}/repos/{owner}/{repo}/pulls/{pr_number}"

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

        Note:
            GitHub provides diff via Accept header on the PR endpoint.
            Using Accept: application/vnd.github.v3.diff returns diff directly.
        """
        url = f"{self.url}/repos/{owner}/{repo}/pulls/{pr_number}"

        # Request diff format using GitHub's media type negotiation
        response = await self.client.get(url, headers={"Accept": "application/vnd.github.v3.diff"})
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

        Note:
            GitHub uses the issues API for PR comments (PRs are special issues).
        """
        url = f"{self.url}/repos/{owner}/{repo}/issues/{pr_number}/comments"
        payload = {"body": body}

        try:
            response = await self.client.post(url, json=payload)
            response.raise_for_status()
        except httpx.HTTPStatusError as e:
            raise ValueError(f"Failed to create PR comment: {e.response.text}")

    async def post_review_comment(
        self,
        owner: str,
        repo: str,
        pr_number: int,
        file_path: str,
        line: int,
        body: str,
    ) -> None:
        """Post a line-specific review comment on a PR (BaseAdapter interface).

        Args:
            owner: Repository owner
            repo: Repository name
            pr_number: Pull request number
            file_path: Path to file being commented on (relative to repo root)
            line: Line number in the file (must be part of PR diff)
            body: Comment body (markdown supported)

        Raises:
            ValueError: If review comment creation fails
            httpx.HTTPStatusError: For HTTP errors

        Note:
            GitHub requires commit_id, line, side, and path fields.
            This method fetches the PR head SHA automatically.
        """
        # Get PR details to extract commit SHA
        pr = await self.get_pr(owner, repo, pr_number)
        commit_sha = pr["head"]["sha"]

        # Post review comment using GitHub's review comments API
        url = f"{self.url}/repos/{owner}/{repo}/pulls/{pr_number}/comments"

        # GitHub requires these fields:
        # - commit_id: SHA of the commit to comment on
        # - path: file path
        # - line: line number
        # - side: "LEFT" (deleted) or "RIGHT" (added)
        # For now, we assume all comments are on added lines (RIGHT)
        payload = {
            "commit_id": commit_sha,
            "path": file_path,
            "line": line,
            "side": "RIGHT",  # GitHub requires explicit side
            "body": body,
        }

        try:
            response = await self.client.post(url, json=payload)
            response.raise_for_status()
        except httpx.HTTPStatusError as e:
            raise ValueError(f"Failed to create review comment: {e.response.text}")

    async def get_file_content(self, owner: str, repo: str, file_path: str, ref: str) -> str:
        """Get file content at a specific git reference.

        Args:
            owner: Repository owner
            repo: Repository name
            file_path: Path to file (relative to repo root)
            ref: Git reference (branch name, tag, or commit SHA)

        Returns:
            File content as string

        Raises:
            ValueError: If file not found
            httpx.HTTPStatusError: For HTTP errors

        Note:
            GitHub returns content base64-encoded in the API response.
        """
        url = f"{self.url}/repos/{owner}/{repo}/contents/{file_path}"
        params = {"ref": ref}

        try:
            response = await self.client.get(url, params=params)
            response.raise_for_status()
            data = response.json()

            # GitHub returns base64-encoded content
            content = data.get("content", "")
            if content:
                # Decode base64 and return as string
                # GitHub may include newlines in the base64, so remove them first
                content = content.replace("\n", "")
                return base64.b64decode(content).decode("utf-8")
            else:
                # Empty file
                return ""
        except httpx.HTTPStatusError as e:
            if e.response.status_code == 404:
                raise ValueError(f"File {file_path} not found at ref {ref}")
            else:
                raise
