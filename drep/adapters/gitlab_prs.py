"""PR/MR data access for the GitLab adapter.

Split from drep/adapters/gitlab.py (file-size limit). Contains MR fetch and
validation, unified-diff reconstruction, and MR summary comments.
"""

import json
import logging

import httpx

from drep.adapters.gitlab_base import GitLabMixinBase

logger = logging.getLogger(__name__)


class GitLabPrMixin(GitLabMixinBase):
    """PR/MR data methods, mixed into GitLabAdapter.

    Host surface (client, api_url, _encode_project_path, _check_rate_limit)
    comes from GitLabMixinBase.
    """

    async def get_pr(self, owner: str, repo: str, pr_number: int) -> dict:
        """Get merge request details.

        Args:
            owner: Project namespace
            repo: Project name
            pr_number: Merge request IID

        Returns:
            MR data dictionary with keys: iid, title, description, state, source_branch,
            target_branch, author, diff_refs (contains base_sha, head_sha, start_sha)

        Raises:
            ValueError: If MR not found (404) or network/API error occurs

        Note:
            GitLab uses "merge requests" not "pull requests", but we keep the
            pr_number parameter name for consistency with BaseAdapter interface.
        """
        project_id = self._encode_project_path(owner, repo)
        url = f"{self.api_url}/projects/{project_id}/merge_requests/{pr_number}"

        try:
            response = await self.client.get(url)
            response.raise_for_status()

            # Validate JSON parsing to handle non-JSON error responses
            try:
                data = response.json()
            except json.JSONDecodeError as exc:
                logger.error(
                    f"GitLab API returned non-JSON response for MR !{pr_number} in {owner}/{repo}",
                    extra={"response_text": response.text[:200]},
                )
                raise ValueError(
                    f"GitLab API returned invalid JSON for {owner}/{repo} MR !{pr_number}: "
                    f"{response.text[:200]}"
                ) from exc

            # Validate required 'diff_refs' field exists
            if "diff_refs" not in data or data["diff_refs"] is None:
                logger.error(
                    f"GitLab response missing 'diff_refs' field for MR !{pr_number} "
                    f"in {owner}/{repo}",
                    extra={"response": data},
                )
                raise ValueError(
                    f"GitLab API response missing 'diff_refs' field for MR !{pr_number} "
                    f"in {owner}/{repo}"
                )

            # Validate required fields within diff_refs
            diff_refs = data["diff_refs"]
            if "base_sha" not in diff_refs:
                logger.error(
                    f"GitLab response missing 'base_sha' in diff_refs for "
                    f"MR !{pr_number} in {owner}/{repo}",
                    extra={"diff_refs": diff_refs},
                )
                raise ValueError(
                    f"GitLab API response missing 'base_sha' in diff_refs for "
                    f"MR !{pr_number} in {owner}/{repo}"
                )

            if "head_sha" not in diff_refs:
                logger.error(
                    f"GitLab response missing 'head_sha' in diff_refs for "
                    f"MR !{pr_number} in {owner}/{repo}",
                    extra={"diff_refs": diff_refs},
                )
                raise ValueError(
                    f"GitLab API response missing 'head_sha' in diff_refs for "
                    f"MR !{pr_number} in {owner}/{repo}"
                )

            logger.debug(
                f"Retrieved MR !{pr_number} from {owner}/{repo}",
                extra={"repo_id": f"{owner}/{repo}", "mr_iid": pr_number},
            )

            # Normalize response to match GitHub/Gitea structure expected by callers
            # Add 'head' field with 'sha' extracted from diff_refs['head_sha']
            # This allows review CLI to use pr['head']['sha'] consistently across platforms
            if "head" not in data:
                data["head"] = {"sha": diff_refs["head_sha"]}

            return data

        # Handle network timeout errors
        except self.NETWORK_ERRORS as exc:
            raise self._network_error(
                exc,
                f"fetching MR !{pr_number} from {owner}/{repo}",
                f"{owner}/{repo}",
                "MR may be very large, or GitLab API is slow.",
            ) from exc
        except httpx.HTTPStatusError as e:
            if e.response.status_code == 404:
                logger.warning(
                    f"Merge request !{pr_number} not found in {owner}/{repo}",
                    extra={"repo_id": f"{owner}/{repo}", "mr_iid": pr_number},
                )
                raise ValueError(f"Merge request !{pr_number} not found in {owner}/{repo}") from e
            # Check for rate limit exceeded
            self._check_rate_limit(e.response, owner, repo)

            logger.error(
                f"HTTP error fetching MR !{pr_number} from {owner}/{repo}: "
                f"{e.response.status_code}",
                extra={
                    "repo_id": f"{owner}/{repo}",
                    "mr_iid": pr_number,
                    "http_status": e.response.status_code,
                    "response_text": e.response.text,
                },
            )
            raise ValueError(
                f"GitLab API error fetching MR !{pr_number} from {owner}/{repo}: {e.response.text}"
            ) from e

    async def get_pr_diff(self, owner: str, repo: str, pr_number: int) -> str:
        """Get merge request diff in unified diff format.

        Args:
            owner: Project namespace
            repo: Project name
            pr_number: Merge request IID

        Returns:
            Unified diff string (can be very large)

        Raises:
            ValueError: If network/API error occurs

        Note:
            GitLab returns diffs as structured JSON array, not unified diff format.
            This method reconstructs the unified diff for compatibility.
        """
        project_id = self._encode_project_path(owner, repo)
        url = f"{self.api_url}/projects/{project_id}/merge_requests/{pr_number}/diffs"

        try:
            response = await self.client.get(url)
            response.raise_for_status()

            # Validate JSON parsing
            try:
                diffs = response.json()
            except json.JSONDecodeError as exc:
                logger.error(
                    f"GitLab API returned non-JSON response for MR !{pr_number} diff",
                    extra={"response_text": response.text[:200]},
                )
                raise ValueError(
                    f"GitLab API returned invalid JSON for {owner}/{repo} MR !{pr_number} diff: "
                    f"{response.text[:200]}"
                ) from exc

            # Validate response is an array (GitLab /diffs endpoint returns array)
            if not isinstance(diffs, list):
                logger.error(
                    f"GitLab API returned non-array response for MR !{pr_number} diff",
                    extra={"response_type": type(diffs).__name__},
                )
                raise ValueError(
                    f"GitLab API response for {owner}/{repo} MR !{pr_number} diff expected array, "
                    f"got {type(diffs).__name__}"
                )

            # Reconstruct unified diff from GitLab's JSON array
            unified_diff = self._reconstruct_unified_diff(diffs)

            logger.debug(
                f"Retrieved diff for MR !{pr_number} from {owner}/{repo}",
                extra={
                    "repo_id": f"{owner}/{repo}",
                    "mr_iid": pr_number,
                    "diff_size": len(unified_diff),
                },
            )

            return unified_diff

        # Handle network timeout errors
        except self.NETWORK_ERRORS as exc:
            raise self._network_error(
                exc,
                f"fetching MR !{pr_number} diff from {owner}/{repo}",
                f"{owner}/{repo}",
                "MR diff may be very large.",
            ) from exc
        except httpx.HTTPStatusError as e:
            # Check for rate limit exceeded
            self._check_rate_limit(e.response, owner, repo)

            logger.error(
                f"HTTP error fetching MR diff for !{pr_number} from {owner}/{repo}: "
                f"{e.response.status_code}",
                extra={
                    "repo_id": f"{owner}/{repo}",
                    "mr_iid": pr_number,
                    "http_status": e.response.status_code,
                },
            )
            raise ValueError(
                f"Failed to fetch MR !{pr_number} diff from {owner}/{repo}: {e.response.text}"
            ) from e

    def _reconstruct_unified_diff(self, diffs: list[dict]) -> str:
        """Reconstruct unified diff from GitLab diff objects.

        GitLab returns diffs as structured JSON, not unified diff format.
        We need to reconstruct it for compatibility.

        Args:
            diffs: List of diff objects from GitLab API

        Returns:
            Unified diff string

        Raises:
            ValueError: If diff objects are malformed or missing required fields

        Example:
            diffs = [
                {
                    "old_path": "file.py",
                    "new_path": "file.py",
                    "diff": "@@ -1,3 +1,4 @@\\n import os\\n+import sys\\n"
                }
            ]
            result = (
                "diff --git a/file.py b/file.py\\n"
                "@@ -1,3 +1,4 @@\\n import os\\n+import sys\\n"
            )
        """
        lines = []
        for i, diff_obj in enumerate(diffs):
            # Validate diff object is a dict
            if not isinstance(diff_obj, dict):
                raise ValueError(
                    f"GitLab API diff object at index {i} is not a dict: "
                    f"got {type(diff_obj).__name__}"
                )

            # Validate required fields exist (paths can be null for new/deleted files)
            if "old_path" not in diff_obj:
                raise ValueError(
                    f"GitLab API diff object at index {i} missing required 'old_path' field"
                )
            if "new_path" not in diff_obj:
                raise ValueError(
                    f"GitLab API diff object at index {i} missing required 'new_path' field"
                )

            # Add file header
            old_path = diff_obj["old_path"]
            new_path = diff_obj["new_path"]
            lines.append(f"diff --git a/{old_path} b/{new_path}")

            # Add diff content (GitLab provides unified diff format in 'diff' field)
            diff_content = diff_obj.get("diff", "")
            if diff_content:
                lines.append(diff_content)

        return "\n".join(lines)

    async def create_pr_comment(self, owner: str, repo: str, pr_number: int, body: str) -> None:
        """Post a general comment on the merge request (not line-specific).

        Args:
            owner: Project namespace
            repo: Project name
            pr_number: Merge request IID
            body: Comment body (markdown supported)

        Raises:
            ValueError: If comment creation fails or network/API error occurs

        Note:
            GitLab uses notes API for MR comments.
        """
        project_id = self._encode_project_path(owner, repo)
        url = f"{self.api_url}/projects/{project_id}/merge_requests/{pr_number}/notes"
        payload = {"body": body}

        try:
            response = await self.client.post(url, json=payload)
            response.raise_for_status()

            # Validate JSON response (defensive - ensure GitLab returned valid data)
            try:
                response.json()
            except json.JSONDecodeError as exc:
                logger.error(
                    f"GitLab API returned non-JSON response for comment on MR !{pr_number}",
                    extra={"response_text": response.text[:200]},
                )
                raise ValueError(
                    f"GitLab API returned invalid JSON after posting comment on "
                    f"MR !{pr_number} in {owner}/{repo}: {response.text[:200]}"
                ) from exc

            logger.debug(
                f"Posted comment on MR !{pr_number} in {owner}/{repo}",
                extra={"repo_id": f"{owner}/{repo}", "mr_iid": pr_number},
            )

        # Handle network timeout errors
        except self.NETWORK_ERRORS as exc:
            raise self._network_error(
                exc,
                f"posting comment on MR !{pr_number} in {owner}/{repo}",
                f"{owner}/{repo}",
            ) from exc
        except httpx.HTTPStatusError as e:
            # Check for rate limit exceeded
            self._check_rate_limit(e.response, owner, repo)

            logger.error(
                f"HTTP error posting comment on MR !{pr_number} in {owner}/{repo}: "
                f"{e.response.status_code}",
                extra={
                    "repo_id": f"{owner}/{repo}",
                    "mr_iid": pr_number,
                    "http_status": e.response.status_code,
                    "response_text": e.response.text,
                },
            )
            raise ValueError(
                f"Failed to create MR comment on !{pr_number} in {owner}/{repo}: {e.response.text}"
            ) from e
