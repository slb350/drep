"""Inline review comment machinery for the GitLab adapter.

Split from drep/adapters/gitlab.py (file-size limit). Contains review-anchor
resolution (MR diff_refs), the one-shot comment entry point, and the canonical
anchored inline-comment primitive with position validation and error handling.
"""

import json
import logging
from typing import TYPE_CHECKING, Any

import httpx

from drep.adapters.base import GitLabReviewAnchor, ReviewAnchor

logger = logging.getLogger(__name__)


class GitLabReviewMixin:
    """Inline review comment methods, mixed into GitLabAdapter.

    Host adapter provides: ``client``, ``api_url``, ``_encode_project_path()``,
    ``_check_rate_limit()``.
    """

    # Host-adapter interface provided by GitLabAdapter (for mypy)
    if TYPE_CHECKING:
        client: httpx.AsyncClient
        api_url: str

        def _encode_project_path(self, owner: str, repo: str) -> str: ...
        def _check_rate_limit(
            self, response: httpx.Response, owner: str = "", repo: str = ""
        ) -> None: ...
        def get_pr(self, owner: str, repo: str, pr_number: int) -> Any: ...

    async def get_review_anchor(self, owner: str, repo: str, pr_number: int) -> GitLabReviewAnchor:
        """Fetch the GitLab review anchor (MR diff_refs) with a single MR fetch.

        GitLab positions inline comments with base/head/start SHAs from the
        MR's diff_refs; a bare commit SHA is not sufficient.

        Args:
            owner: Project namespace
            repo: Project name
            pr_number: Merge request IID

        Returns:
            GitLabReviewAnchor carrying base/head/start SHAs

        Raises:
            ValueError: If diff_refs or any required SHA is missing
        """
        mr_data = await self.get_pr(owner, repo, pr_number)

        diff_refs = mr_data.get("diff_refs")
        if not diff_refs:
            logger.error(
                f"MR !{pr_number} response missing diff_refs in {owner}/{repo}",
                extra={"repo_id": f"{owner}/{repo}", "mr_iid": pr_number, "mr_data": mr_data},
            )
            raise ValueError(
                f"GitLab API response for MR !{pr_number} in {owner}/{repo} "
                "missing required 'diff_refs' field. MR may not have commits yet."
            )

        required_shas = ["base_sha", "head_sha", "start_sha"]
        for sha_field in required_shas:
            if sha_field not in diff_refs:
                logger.error(
                    f"MR !{pr_number} diff_refs missing {sha_field} in {owner}/{repo}",
                    extra={
                        "repo_id": f"{owner}/{repo}",
                        "mr_iid": pr_number,
                        "diff_refs": diff_refs,
                    },
                )
                raise ValueError(
                    f"GitLab API response for MR !{pr_number} in {owner}/{repo} "
                    f"missing required '{sha_field}' in diff_refs"
                )

        return GitLabReviewAnchor(
            owner=owner,
            repo=repo,
            pr_number=pr_number,
            commit_sha=diff_refs["head_sha"],
            base_sha=diff_refs["base_sha"],
            start_sha=diff_refs["start_sha"],
        )

    async def post_review_comment(
        self,
        owner: str,
        repo: str,
        pr_number: int,
        file_path: str,
        line: int,
        body: str,
    ) -> None:
        """Post a line-specific review comment on a merge request.

        Resolves the GitLab review anchor (MR diff_refs), then delegates to
        ``create_pr_review_comment``.

        Args:
            owner: Project namespace
            repo: Project name
            pr_number: Merge request IID
            file_path: Path to file being commented on (relative to repo root)
            line: Line number in the file (must be part of MR diff)
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
            anchor: GitLabReviewAnchor carrying MR diff_refs (base/head/start SHAs)
            file_path: File path relative to repo root
            line: Line number in new version (after changes)
            body: Comment body (markdown supported)

        Raises:
            ValueError: If the anchor lacks diff_refs, or comment creation fails
        """
        if (
            not isinstance(anchor, GitLabReviewAnchor)
            or not anchor.base_sha
            or not anchor.start_sha
        ):
            raise ValueError(
                "GitLab inline comments require a GitLabReviewAnchor with "
                "base_sha/head_sha/start_sha from get_review_anchor()"
            )

        owner, repo, pr_number = anchor.owner, anchor.repo, anchor.pr_number

        project_id = self._encode_project_path(owner, repo)
        url = f"{self.api_url}/projects/{project_id}/merge_requests/{pr_number}/discussions"

        # Build position object (GitLab-specific format for inline comments)
        position = {
            "base_sha": anchor.base_sha,
            "start_sha": anchor.start_sha,
            "head_sha": anchor.commit_sha,
            "position_type": "text",  # Can be 'text' or 'image'
            "new_path": file_path,
            "new_line": line,
        }

        payload = {"body": body, "position": position}

        try:
            response = await self.client.post(url, json=payload)
            response.raise_for_status()

            # Validate JSON response (defensive - ensure GitLab returned valid data)
            try:
                response.json()
            except json.JSONDecodeError as exc:
                logger.error(
                    f"GitLab API returned non-JSON response for review comment on MR !{pr_number}",
                    extra={"response_text": response.text[:200]},
                )
                raise ValueError(
                    f"GitLab API returned invalid JSON after posting review comment on "
                    f"MR !{pr_number} in {owner}/{repo}: {response.text[:200]}"
                ) from exc

            logger.debug(
                f"Posted inline comment on MR !{pr_number} in {owner}/{repo} at {file_path}:{line}",
                extra={
                    "repo_id": f"{owner}/{repo}",
                    "mr_iid": pr_number,
                    "file_path": file_path,
                    "line": line,
                },
            )

        # Handle network timeout errors
        except httpx.TimeoutException as exc:
            logger.error(
                f"Timeout posting review comment on MR !{pr_number} in {owner}/{repo}",
                extra={
                    "repo_id": f"{owner}/{repo}",
                    "mr_iid": pr_number,
                    "file_path": file_path,
                    "line": line,
                },
            )
            raise ValueError(
                f"GitLab API request timed out after {self.client.timeout.read}s "
                f"posting review comment on MR !{pr_number} in {owner}/{repo}."
            ) from exc
        except (httpx.ConnectError, httpx.ConnectTimeout) as exc:
            logger.error(
                f"Failed to connect to GitLab API for {owner}/{repo}",
                extra={"repo_id": f"{owner}/{repo}"},
            )
            raise ValueError(
                f"Cannot connect to GitLab API at {self.api_url} for {owner}/{repo}. "
                "Check your internet connection, firewall, or GitLab API status."
            ) from exc
        except httpx.HTTPStatusError as e:
            # Check for rate limit exceeded
            self._check_rate_limit(e.response, owner, repo)

            # Handle 400 (Bad Request) - likely invalid position or line number
            if e.response.status_code == 400:
                logger.warning(
                    f"Invalid position for review comment on MR !{pr_number} in {owner}/{repo}",
                    extra={
                        "repo_id": f"{owner}/{repo}",
                        "mr_iid": pr_number,
                        "file_path": file_path,
                        "line": line,
                        "response_text": e.response.text,
                    },
                )
                raise ValueError(
                    f"Invalid position for review comment on MR !{pr_number} "
                    f"in {owner}/{repo} at {file_path}:{line}. Line must be part of MR diff. "
                    f"GitLab error: {e.response.text}"
                ) from e

            logger.error(
                f"HTTP error posting review comment on MR !{pr_number} in {owner}/{repo}: "
                f"{e.response.status_code}",
                extra={
                    "repo_id": f"{owner}/{repo}",
                    "mr_iid": pr_number,
                    "http_status": e.response.status_code,
                    "response_text": e.response.text,
                },
            )
            raise ValueError(
                f"Failed to create review comment on MR !{pr_number} in {owner}/{repo}: "
                f"{e.response.text}"
            ) from e
