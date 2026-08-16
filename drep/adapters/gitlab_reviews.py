"""Inline review comment machinery for the GitLab adapter.

Split from drep/adapters/gitlab.py (file-size limit). Contains review-anchor
resolution (MR diff_refs), the one-shot comment entry point, and the canonical
anchored inline-comment primitive with position validation and error handling.
"""

import json
import logging
from dataclasses import dataclass

import httpx

from drep.adapters.base import ReviewAnchor
from drep.adapters.gitlab_base import GitLabMixinBase

logger = logging.getLogger(__name__)


@dataclass(frozen=True)
class GitLabReviewAnchor(ReviewAnchor):
    """GitLab-specific anchor carrying MR diff_refs SHAs.

    GitLab positions inline comments with base/head/start SHAs from the MR's
    ``diff_refs``; a bare commit SHA is not sufficient. Both fields are
    required: defaulting them would manufacture the invalid state that
    ``create_pr_review_comment`` would then have to guard against.

    Lives here rather than in base.py so the platform-neutral contract stays
    free of any one platform's positioning model.
    """

    base_sha: str
    start_sha: str


class GitLabReviewMixin(GitLabMixinBase):
    """Inline review comment methods, mixed into GitLabAdapter.

    Host surface (client, api_url, _encode_project_path, _check_rate_limit)
    comes from GitLabMixinBase.
    """

    def anchor_from_pr(
        self, pr_data: dict, owner: str, repo: str, pr_number: int
    ) -> GitLabReviewAnchor:
        """Derive the GitLab review anchor (MR diff_refs) from an MR payload.

        GitLab positions inline comments with base/head/start SHAs from the
        MR's diff_refs; a bare commit SHA is not sufficient.

        Args:
            pr_data: MR payload as returned by ``get_pr``
            owner: Project namespace
            repo: Project name
            pr_number: Merge request IID

        Returns:
            GitLabReviewAnchor carrying base/head/start SHAs

        Raises:
            ValueError: If diff_refs or any required SHA is missing
        """
        mr_data = pr_data

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
        if not isinstance(anchor, GitLabReviewAnchor):
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
        except self.NETWORK_ERRORS as exc:
            raise self._network_error(
                exc,
                f"posting review comment on MR !{pr_number} in {owner}/{repo}",
                f"{owner}/{repo}",
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
