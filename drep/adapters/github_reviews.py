"""Inline review comment machinery for the GitHub adapter.

Split from drep/adapters/github.py (file-size limit). post_review_comment
resolves the anchor and delegates; create_pr_review_comment is the canonical
primitive and owns the 422 invalid-line handling.
"""

import logging
from typing import TYPE_CHECKING, Any

import httpx

from drep.adapters.base import ReviewAnchor

logger = logging.getLogger(__name__)


class GitHubReviewMixin:
    """Inline review comment methods, mixed into GitHubAdapter.

    Host adapter provides: ``client``, ``url``, ``_check_rate_limit()``.
    """

    if TYPE_CHECKING:
        client: httpx.AsyncClient
        url: str

        def _check_rate_limit(
            self, response: httpx.Response, owner: str = "", repo: str = ""
        ) -> None: ...
        def get_review_anchor(self, owner: str, repo: str, pr_number: int) -> Any: ...

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

        Resolves the review anchor (head commit SHA), then delegates to
        ``create_pr_review_comment``.

        Args:
            owner: Repository owner
            repo: Repository name
            pr_number: Pull request number
            file_path: Path to file being commented on (relative to repo root)
            line: Line number in the file (must be part of PR diff)
            body: Comment body (markdown supported)

        Raises:
            ValueError: If review comment creation fails or network/API error occurs
        """
        anchor = await self.get_review_anchor(owner, repo, pr_number)
        await self.create_pr_review_comment(anchor, file_path, line, body)

    async def create_pr_review_comment(
        self,
        anchor: ReviewAnchor,
        file_path: str,
        line: int,
        body: str,
    ) -> None:
        """Post an inline review comment using a pre-fetched review anchor.

        Args:
            anchor: Immutable review anchor (from get_review_anchor)
            file_path: File path relative to repo root
            line: Line number in new version (after changes)
            body: Comment body (markdown supported)

        Raises:
            ValueError: If review comment creation fails or network/API error occurs

        Note:
            Implementation currently only supports comments on added/modified lines
            (side="RIGHT"). Comments on deleted lines are not supported. This is
            consistent with drep's current usage pattern of only commenting on
            added code.
        """
        owner, repo, pr_number = anchor.owner, anchor.repo, anchor.pr_number

        # Post review comment using GitHub's review comments API
        url = f"{self.url}/repos/{owner}/{repo}/pulls/{pr_number}/comments"

        # GitHub requires these fields:
        # - commit_id: SHA of the commit to comment on
        # - path: file path
        # - line: line number
        # - side: "LEFT" (deleted) or "RIGHT" (added)
        # Assumption: Only support comments on added lines (side="RIGHT"), not deleted lines
        payload = {
            "commit_id": anchor.commit_sha,
            "path": file_path,
            "line": line,
            "side": "RIGHT",  # GitHub requires explicit side
            "body": body,
        }

        try:
            response = await self.client.post(url, json=payload)
            response.raise_for_status()

            logger.debug(
                f"Posted review comment on PR #{pr_number} in {owner}/{repo} at {file_path}:{line}",
                extra={
                    "repo_id": f"{owner}/{repo}",
                    "pr_number": pr_number,
                    "file_path": file_path,
                    "line": line,
                },
            )

        # Handle network timeout errors
        except httpx.TimeoutException as exc:
            logger.error(
                f"Timeout posting review comment on PR #{pr_number} in {owner}/{repo}",
                extra={
                    "repo_id": f"{owner}/{repo}",
                    "pr_number": pr_number,
                    "file_path": file_path,
                    "line": line,
                },
            )
            raise ValueError(
                f"GitHub API request timed out after {self.client.timeout.read}s "
                f"posting review comment on PR #{pr_number} in {owner}/{repo}."
            ) from exc
        except (httpx.ConnectError, httpx.ConnectTimeout) as exc:
            logger.error(
                f"Failed to connect to GitHub API for {owner}/{repo}",
                extra={"repo_id": f"{owner}/{repo}"},
            )
            raise ValueError(
                f"Cannot connect to GitHub API at {self.url} for {owner}/{repo}. "
                "Check your internet connection, firewall, or GitHub API status."
            ) from exc
        except httpx.HTTPStatusError as e:
            # Check for rate limit exceeded
            self._check_rate_limit(e.response, owner, repo)

            # Handle 422 (Validation Failed) - likely invalid line number
            if e.response.status_code == 422:
                logger.warning(
                    f"Invalid line number {line} for review comment on "
                    f"PR #{pr_number} in {owner}/{repo}",
                    extra={
                        "repo_id": f"{owner}/{repo}",
                        "pr_number": pr_number,
                        "file_path": file_path,
                        "line": line,
                        "response_text": e.response.text,
                    },
                )
                raise ValueError(
                    f"Invalid line number {line} for review comment on PR #{pr_number} "
                    f"in {owner}/{repo} at {file_path}. Line must be part of PR diff. "
                    f"GitHub error: {e.response.text}"
                ) from e

            logger.error(
                f"HTTP error posting review comment on PR #{pr_number} in {owner}/{repo}: "
                f"{e.response.status_code}",
                extra={
                    "repo_id": f"{owner}/{repo}",
                    "pr_number": pr_number,
                    "http_status": e.response.status_code,
                    "response_text": e.response.text,
                },
            )
            raise ValueError(
                f"Failed to create review comment on PR #{pr_number} in {owner}/{repo}: "
                f"{e.response.text}"
            ) from e
