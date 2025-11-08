"""Abstract base class for platform adapters (Gitea, GitHub, GitLab).

This module defines the common interface that all platform adapters must implement.
By using an abstract base class, we ensure:
- Compile-time verification of interface compliance
- Consistent API across all platforms
- Better IDE autocomplete and type checking
- Easier to add new platform adapters
"""

from abc import ABC, abstractmethod
from typing import Dict, List, Optional


class BaseAdapter(ABC):
    """Abstract base class for git platform adapters.

    All platform adapters (Gitea, GitHub, GitLab, etc.) must inherit from this
    class and implement all abstract methods. This ensures a consistent interface
    for interacting with different git hosting platforms.

    The adapter pattern allows drep to work with multiple platforms without
    tying the core logic to any specific platform's API.
    """

    @abstractmethod
    async def create_issue(
        self,
        owner: str,
        repo: str,
        title: str,
        body: str,
        labels: Optional[List[str]] = None,
    ) -> int:
        """Create an issue on the platform and return its number/ID.

        Args:
            owner: Repository owner (username or organization)
            repo: Repository name
            title: Issue title
            body: Issue body (markdown supported)
            labels: Optional list of label names to apply

        Returns:
            Issue number/ID of the created issue

        Raises:
            ValueError: If issue creation fails or labels are invalid
            httpx.HTTPStatusError: For HTTP errors (rate limits, auth failures)

        Example:
            issue_num = await adapter.create_issue(
                owner="user",
                repo="project",
                title="Bug: Login broken",
                body="Users can't log in...",
                labels=["bug", "high-priority"],
            )
        """
        pass

    @abstractmethod
    async def get_pr(self, owner: str, repo: str, pr_number: int) -> Dict:
        """Get pull request details.

        Args:
            owner: Repository owner
            repo: Repository name
            pr_number: Pull request number

        Returns:
            Dictionary with PR data including keys:
            - number: PR number
            - title: PR title
            - body: PR description
            - state: PR state (open, closed, merged)
            - base: Base branch information
            - head: Head branch information
            - user: PR author information

        Raises:
            ValueError: If PR not found
            httpx.HTTPStatusError: For other HTTP errors

        Example:
            pr = await adapter.get_pr(owner="user", repo="project", pr_number=42)
            print(f"PR #{pr['number']}: {pr['title']}")
        """
        pass

    @abstractmethod
    async def get_pr_diff(self, owner: str, repo: str, pr_number: int) -> str:
        """Get pull request diff in unified diff format.

        Args:
            owner: Repository owner
            repo: Repository name
            pr_number: Pull request number

        Returns:
            Unified diff string showing all changes in the PR

        Raises:
            httpx.HTTPStatusError: For HTTP errors

        Note:
            Diff can be very large for PRs with many changes. Consider
            streaming or pagination for production use with large PRs.

        Example:
            diff = await adapter.get_pr_diff(owner="user", repo="project", pr_number=42)
            print(f"Diff size: {len(diff)} bytes")
        """
        pass

    @abstractmethod
    async def create_pr_comment(self, owner: str, repo: str, pr_number: int, body: str) -> None:
        """Post a general comment on the PR (not line-specific).

        Args:
            owner: Repository owner
            repo: Repository name
            pr_number: Pull request number
            body: Comment body (markdown supported)

        Raises:
            httpx.HTTPStatusError: For HTTP errors

        Example:
            await adapter.create_pr_comment(
                owner="user",
                repo="project",
                pr_number=42,
                body="LGTM! Ready to merge.",
            )
        """
        pass

    @abstractmethod
    async def post_review_comment(
        self,
        owner: str,
        repo: str,
        pr_number: int,
        file_path: str,
        line: int,
        body: str,
    ) -> None:
        """Post a line-specific review comment on a PR.

        Args:
            owner: Repository owner
            repo: Repository name
            pr_number: Pull request number
            file_path: Path to file being commented on (relative to repo root)
            line: Line number in the file (must be part of PR diff)
            body: Comment body (markdown supported)

        Raises:
            ValueError: If line number is invalid (not in PR diff)
            httpx.HTTPStatusError: For HTTP errors

        Note:
            Line numbers must correspond to lines visible in the PR diff.
            Different platforms may have different validation rules for line numbers.

        Example:
            await adapter.post_review_comment(
                owner="user",
                repo="project",
                pr_number=42,
                file_path="src/main.py",
                line=15,
                body="Consider using a with statement here",
            )
        """
        pass

    @abstractmethod
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

        Example:
            content = await adapter.get_file_content(
                owner="user",
                repo="project",
                file_path="README.md",
                ref="main",
            )
        """
        pass

    @abstractmethod
    async def get_default_branch(self, owner: str, repo: str) -> str:
        """Get repository default branch name.

        Args:
            owner: Repository owner
            repo: Repository name

        Returns:
            Default branch name (e.g., "main", "master", "develop")

        Raises:
            ValueError: If repository not found or network/API error occurs

        Example:
            branch = await adapter.get_default_branch(owner="user", repo="project")
            # Returns: "main"
        """
        pass

    @abstractmethod
    async def close(self) -> None:
        """Close the adapter and release resources (HTTP connections, etc.).

        Should be called when the adapter is no longer needed to properly
        clean up HTTP clients and other resources.

        Example:
            adapter = GiteaAdapter(url="...", token="...")
            try:
                # Use adapter
                await adapter.create_issue(...)
            finally:
                await adapter.close()
        """
        pass
