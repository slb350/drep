"""GitLab platform adapter implementation.

Implements BaseAdapter interface for GitLab.com and self-hosted GitLab instances.
Uses GitLab REST API v4.

Key Differences from GitHub:
- Uses "merge requests" (MR) instead of "pull requests" (PR)
- Authentication via PRIVATE-TOKEN header (not Bearer)
- Inline comments via discussions with position objects
- File content is base64 encoded
- Project IDs are URL-encoded (owner%2Frepo)
- Labels are comma-separated strings, not arrays
- Uses 'description' for issue body, not 'body'
"""

import asyncio
import base64
import json
import logging
import urllib.parse
from typing import Dict, List, Optional

import httpx

from drep.adapters.base import BaseAdapter

logger = logging.getLogger(__name__)


class GitLabAdapter(BaseAdapter):
    """GitLab API adapter.

    Supports GitLab.com and self-hosted GitLab instances.
    Uses GitLab REST API v4.

    Authentication:
        - GitLab: 'PRIVATE-TOKEN: {token}' header
        - GitHub: 'Authorization: Bearer {token}' header

    Project Path Encoding:
        - All project paths (owner/repo) must be URL-encoded
        - Example: owner/repo → owner%2Frepo

    Merge Requests vs Pull Requests:
        - GitLab uses "merge requests" (MR) with IID (internal ID)
        - GitHub uses "pull requests" (PR) with number
        - For consistency, we keep pr_number parameter names

    Inline Comments:
        - GitLab uses discussions with position objects
        - Requires base_sha, head_sha, start_sha from MR diff_refs
        - GitHub uses review comments with line + side fields
    """

    def __init__(self, token: str, url: Optional[str] = None):
        """Initialize GitLabAdapter with token.

        Args:
            token: GitLab Personal Access Token (requires api scope) as plain string.
                   IMPORTANT: If loading from GitLabConfig (Pydantic model), you must
                   unwrap SecretStr by calling: config.gitlab.token.get_secret_value()
            url: GitLab base URL (None = gitlab.com, else full URL like https://gitlab.example.com).
                 The /api/v4 suffix is optional - it will be stripped if present and re-added
                 automatically to prevent URL duplication.

        Raises:
            ValueError: If token is empty

        Example:
            # GitLab.com (default)
            adapter = GitLabAdapter(token="glpat-...")
            try:
                issue_num = await adapter.create_issue("owner", "repo", "Title", "Body")
            finally:
                await adapter.close()

            # Self-hosted GitLab
            adapter = GitLabAdapter(
                token="glpat-...",
                url="https://gitlab.company.com"
            )

            # Loading from GitLabConfig (CRITICAL: unwrap SecretStr)
            from drep.config import load_config

            config = load_config("config.yaml")
            if config.gitlab:
                adapter = GitLabAdapter(
                    token=config.gitlab.token.get_secret_value(),  # Unwrap SecretStr!
                    url=config.gitlab.url  # Already a string or None
                )
                try:
                    # Use adapter...
                    pass
                finally:
                    await adapter.close()

        Note:
            Always call close() when done to release HTTP client resources.
            Use try/finally or async context manager pattern to ensure cleanup.
        """
        # Validate token is not empty or whitespace
        if not token or not token.strip():
            raise ValueError("GitLab token cannot be empty")

        # Default to GitLab.com
        if url is None:
            self.base_url = "https://gitlab.com"
        else:
            # Validate URL starts with http:// or https://
            if not url.startswith(("http://", "https://")):
                raise ValueError(f"GitLab URL must start with http:// or https://, got: {url}")

            # Strip trailing slashes and /api/v4 suffix if present
            # This prevents URL duplication like https://gitlab.com/api/v4/api/v4/...
            clean_url = url.rstrip("/")
            if clean_url.endswith("/api/v4"):
                clean_url = clean_url[:-7]  # Remove "/api/v4"
            self.base_url = clean_url

        self.api_url = f"{self.base_url}/api/v4"
        self.token = token.strip()

        # GitLab uses PRIVATE-TOKEN header (NOT Authorization: Bearer!)
        self.client = httpx.AsyncClient(
            headers={
                "PRIVATE-TOKEN": self.token,
                "Accept": "application/json",
            },
            timeout=30.0,
        )

        logger.debug("Initialized GitLab adapter", extra={"api_url": self.api_url, "timeout": 30.0})

    async def close(self):
        """Close HTTP client connection.

        Note:
            Non-critical errors during close are logged but not re-raised to avoid
            masking original errors in finally blocks. Critical exceptions
            (KeyboardInterrupt, SystemExit, asyncio.CancelledError) are always
            propagated.
        """
        try:
            await self.client.aclose()
            logger.debug("Closed GitLab adapter HTTP client")
        except (KeyboardInterrupt, SystemExit, asyncio.CancelledError):
            # Always propagate user interrupts, system exit signals, and async cancellations
            logger.info("Close interrupted by user or system")
            raise
        except (httpx.CloseError, RuntimeError) as e:
            # Expected errors during close - suppress to avoid masking original errors
            logger.warning(
                f"Non-critical error closing GitLab client: {e}",
                extra={"error_type": type(e).__name__},
            )
        except Exception as e:
            # Unexpected errors - log at ERROR level with full traceback for debugging
            logger.error(
                f"Unexpected error closing GitLab adapter: {e}",
                extra={"error_type": type(e).__name__},
                exc_info=True,
            )

    def _encode_project_path(self, owner: str, repo: str) -> str:
        """Encode project path for GitLab API.

        GitLab APIs require namespace/project to be URL-encoded.
        Example: owner/repo → owner%2Frepo

        Args:
            owner: Project namespace/owner
            repo: Project name

        Returns:
            URL-encoded project path

        Example:
            _encode_project_path("myorg", "myrepo") → "myorg%2Fmyrepo"
        """
        project_path = f"{owner}/{repo}"
        return urllib.parse.quote(project_path, safe="")

    def _check_rate_limit(self, response: httpx.Response, owner: str = "", repo: str = "") -> None:
        """Check for rate limit and raise informative error.

        Args:
            response: HTTP response from GitLab API
            owner: Repository owner (for error context)
            repo: Repository name (for error context)

        Raises:
            ValueError: If rate limit is exceeded (429 status)

        Note:
            GitLab returns rate limit info in RateLimit-* headers:
            - RateLimit-Limit: Maximum requests per time window
            - RateLimit-Remaining: Requests remaining
            - RateLimit-Reset: Unix timestamp when limit resets

            If we get a 429 status, we ALWAYS raise an error, regardless of
            what the headers say (they might be malformed or inconsistent).
        """
        if response.status_code != 429:
            return  # Not a rate limit error

        # If we got 429, we're rate limited - always raise
        # Parse headers for better error message, but don't depend on them
        reset_time_raw = response.headers.get("RateLimit-Reset", "unknown")

        # Convert Unix timestamp to human-readable format
        if reset_time_raw != "unknown":
            try:
                from datetime import datetime

                reset_dt = datetime.fromtimestamp(int(reset_time_raw))
                reset_time = reset_dt.strftime("%Y-%m-%d %H:%M:%S UTC")
            except (ValueError, OverflowError, OSError):
                # Invalid timestamp - truncate if too long
                reset_time = str(reset_time_raw)[:50]
        else:
            reset_time = "unknown"

        context = f" for {owner}/{repo}" if owner and repo else ""
        repo_id = f"{owner}/{repo}" if owner and repo else None

        logger.warning(
            f"GitLab API rate limit exceeded{context}",
            extra={
                "repo_id": repo_id,
                "reset_time": reset_time,
                "reset_time_raw": reset_time_raw,
            },
        )

        raise ValueError(
            f"GitLab API rate limit exceeded (HTTP 429). "
            f"Resets at {reset_time}. "
            "Wait and retry, or use a different token."
        )

    # ===== Repository Methods =====

    async def get_default_branch(self, owner: str, repo: str) -> str:
        """Get repository default branch name.

        Args:
            owner: Repository owner (namespace)
            repo: Repository name

        Returns:
            Default branch name (e.g., "main", "master", "develop")

        Raises:
            ValueError: If repository not found (404) or network/API error occurs

        Example:
            branch = await adapter.get_default_branch("owner", "repo")
            # Returns: "main"
        """
        project_id = self._encode_project_path(owner, repo)
        url = f"{self.api_url}/projects/{project_id}"

        try:
            response = await self.client.get(url)
            response.raise_for_status()

            # Validate JSON parsing to handle non-JSON error responses
            try:
                data = response.json()
            except json.JSONDecodeError:
                logger.error(
                    f"GitLab API returned non-JSON response for {owner}/{repo}",
                    extra={"response_text": response.text[:200]},
                )
                raise ValueError(
                    f"GitLab API returned invalid JSON for {owner}/{repo}: "
                    f"{response.text[:200]}"
                )

            # Validate required 'default_branch' field exists in API response
            if "default_branch" not in data:
                logger.error(
                    f"GitLab response missing 'default_branch' field for {owner}/{repo}",
                    extra={"response": data},
                )
                raise ValueError(
                    f"GitLab API response missing 'default_branch' field for {owner}/{repo}"
                )

            default_branch = data["default_branch"]

            logger.debug(
                f"Retrieved default branch '{default_branch}' for {owner}/{repo}",
                extra={"repo_id": f"{owner}/{repo}", "default_branch": default_branch},
            )

            return default_branch

        # Handle network timeout errors
        except httpx.TimeoutException:
            logger.error(
                f"Timeout fetching default branch for {owner}/{repo}",
                extra={"repo_id": f"{owner}/{repo}"},
            )
            raise ValueError(
                f"GitLab API request timed out after {self.client.timeout.read}s "
                f"fetching default branch for {owner}/{repo}. "
                "Project may be very large, or GitLab API is slow."
            )
        except (httpx.ConnectError, httpx.ConnectTimeout):
            logger.error(
                f"Failed to connect to GitLab API for {owner}/{repo}",
                extra={"repo_id": f"{owner}/{repo}"},
            )
            raise ValueError(
                f"Cannot connect to GitLab API at {self.api_url} for {owner}/{repo}. "
                "Check your internet connection, firewall, or GitLab API status."
            )
        except httpx.HTTPStatusError as e:
            if e.response.status_code == 404:
                logger.warning(
                    f"Project {owner}/{repo} not found",
                    extra={"repo_id": f"{owner}/{repo}"},
                )
                raise ValueError(f"GitLab project {owner}/{repo} not found")
            else:
                # Check for rate limit exceeded
                self._check_rate_limit(e.response, owner, repo)

                logger.error(
                    f"HTTP error fetching default branch for {owner}/{repo}: "
                    f"{e.response.status_code}",
                    extra={
                        "repo_id": f"{owner}/{repo}",
                        "http_status": e.response.status_code,
                        "response_text": e.response.text,
                    },
                )
                raise ValueError(
                    f"GitLab API error fetching default branch for {owner}/{repo}: "
                    f"{e.response.text}"
                )

    # ===== Issue Methods =====

    async def create_issue(
        self, owner: str, repo: str, title: str, body: str, labels: Optional[List[str]] = None
    ) -> int:
        """Create an issue and return issue IID (internal ID).

        Args:
            owner: Project namespace
            repo: Project name
            title: Issue title
            body: Issue body (markdown supported)
            labels: Optional list of label names

        Returns:
            Created issue IID (internal ID, not global ID)

        Raises:
            ValueError: If issue creation fails due to:
                - Network timeout (request exceeds 30s timeout)
                - Connection failure (cannot reach GitLab API)
                - GitLab API rate limit exceeded
                - Invalid JSON response from GitLab API
                - Missing required 'iid' field in API response
                - HTTP errors (401 Unauthorized, 403 Forbidden, 500 Server Error, etc.)

        Note:
            GitLab uses 'description' for issue body (not 'body' like GitHub).
            Labels must be comma-separated string (not array).
        """
        project_id = self._encode_project_path(owner, repo)
        url = f"{self.api_url}/projects/{project_id}/issues"

        # GitLab uses 'description' not 'body'!
        payload = {"title": title, "description": body}

        # GitLab labels are comma-separated string (not array!)
        if labels:
            payload["labels"] = ",".join(labels)

        try:
            response = await self.client.post(url, json=payload)
            response.raise_for_status()

            # Validate JSON parsing to handle non-JSON error responses
            try:
                data = response.json()
            except json.JSONDecodeError:
                logger.error(
                    f"GitLab API returned non-JSON response for {owner}/{repo}",
                    extra={"response_text": response.text[:200]},
                )
                raise ValueError(
                    f"GitLab API returned invalid JSON for {owner}/{repo}: "
                    f"{response.text[:200]}"
                )

            # Validate required 'iid' field exists in API response (use IID not global ID)
            if "iid" not in data:
                logger.error(
                    f"GitLab response missing 'iid' field for {owner}/{repo}",
                    extra={"response": data},
                )
                raise ValueError(f"GitLab API response missing 'iid' field for {owner}/{repo}")

            issue_iid = data["iid"]

            # Log successful issue creation with context
            logger.debug(
                f"Created issue #{issue_iid} in {owner}/{repo}",
                extra={"repo_id": f"{owner}/{repo}", "issue_iid": issue_iid},
            )

            return issue_iid

        # Handle network timeout errors
        except httpx.TimeoutException:
            logger.error(
                f"Timeout creating issue in {owner}/{repo}",
                extra={"repo_id": f"{owner}/{repo}", "timeout": self.client.timeout.read},
            )
            raise ValueError(
                f"GitLab API request timed out after {self.client.timeout.read}s "
                f"for {owner}/{repo}. GitLab API may be slow or project may be large."
            )
        except (httpx.ConnectError, httpx.ConnectTimeout):
            logger.error(
                f"Failed to connect to GitLab API for {owner}/{repo}",
                extra={"repo_id": f"{owner}/{repo}", "api_url": self.api_url},
            )
            raise ValueError(
                f"Cannot connect to GitLab API at {self.api_url} for {owner}/{repo}. "
                "Check your internet connection, firewall, or GitLab API status."
            )
        except httpx.HTTPStatusError as e:
            # Check for rate limit exceeded
            self._check_rate_limit(e.response, owner, repo)

            # Include project context in error message for debugging
            logger.error(
                f"HTTP error creating issue in {owner}/{repo}: {e.response.status_code}",
                extra={
                    "repo_id": f"{owner}/{repo}",
                    "http_status": e.response.status_code,
                    "response_text": e.response.text,
                },
            )
            raise ValueError(f"Failed to create issue in {owner}/{repo}: {e.response.text}")

    # ===== MR Review Methods =====

    async def get_pr(self, owner: str, repo: str, pr_number: int) -> Dict:
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
            except json.JSONDecodeError:
                logger.error(
                    f"GitLab API returned non-JSON response for MR !{pr_number} in {owner}/{repo}",
                    extra={"response_text": response.text[:200]},
                )
                raise ValueError(
                    f"GitLab API returned invalid JSON for {owner}/{repo} MR !{pr_number}: "
                    f"{response.text[:200]}"
                )

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
        except httpx.TimeoutException:
            logger.error(
                f"Timeout fetching MR !{pr_number} from {owner}/{repo}",
                extra={"repo_id": f"{owner}/{repo}", "mr_iid": pr_number},
            )
            raise ValueError(
                f"GitLab API request timed out after {self.client.timeout.read}s "
                f"fetching MR !{pr_number} from {owner}/{repo}. "
                "MR may be very large, or GitLab API is slow."
            )
        except (httpx.ConnectError, httpx.ConnectTimeout):
            logger.error(
                f"Failed to connect to GitLab API for {owner}/{repo}",
                extra={"repo_id": f"{owner}/{repo}"},
            )
            raise ValueError(
                f"Cannot connect to GitLab API at {self.api_url} for {owner}/{repo}. "
                "Check your internet connection, firewall, or GitLab API status."
            )
        except httpx.HTTPStatusError as e:
            if e.response.status_code == 404:
                logger.warning(
                    f"Merge request !{pr_number} not found in {owner}/{repo}",
                    extra={"repo_id": f"{owner}/{repo}", "mr_iid": pr_number},
                )
                raise ValueError(f"Merge request !{pr_number} not found in {owner}/{repo}")
            else:
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
                    f"GitLab API error fetching MR !{pr_number} from {owner}/{repo}: "
                    f"{e.response.text}"
                )

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
            except json.JSONDecodeError:
                logger.error(
                    f"GitLab API returned non-JSON response for MR !{pr_number} diff",
                    extra={"response_text": response.text[:200]},
                )
                raise ValueError(
                    f"GitLab API returned invalid JSON for {owner}/{repo} MR !{pr_number} diff: "
                    f"{response.text[:200]}"
                )

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
        except httpx.TimeoutException:
            logger.error(
                f"Timeout fetching MR diff for !{pr_number} from {owner}/{repo}",
                extra={"repo_id": f"{owner}/{repo}", "mr_iid": pr_number},
            )
            raise ValueError(
                f"GitLab API request timed out after {self.client.timeout.read}s "
                f"fetching MR !{pr_number} diff from {owner}/{repo}. MR diff may be very large."
            )
        except (httpx.ConnectError, httpx.ConnectTimeout):
            logger.error(
                f"Failed to connect to GitLab API for {owner}/{repo}",
                extra={"repo_id": f"{owner}/{repo}"},
            )
            raise ValueError(
                f"Cannot connect to GitLab API at {self.api_url} for {owner}/{repo}. "
                "Check your internet connection, firewall, or GitLab API status."
            )
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
            )

    def _reconstruct_unified_diff(self, diffs: List[Dict]) -> str:
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
                    f"GitLab API diff object at index {i} missing required " f"'old_path' field"
                )
            if "new_path" not in diff_obj:
                raise ValueError(
                    f"GitLab API diff object at index {i} missing required " f"'new_path' field"
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
            except json.JSONDecodeError:
                logger.error(
                    f"GitLab API returned non-JSON response for comment on MR !{pr_number}",
                    extra={"response_text": response.text[:200]},
                )
                raise ValueError(
                    f"GitLab API returned invalid JSON after posting comment on "
                    f"MR !{pr_number} in {owner}/{repo}: {response.text[:200]}"
                )

            logger.debug(
                f"Posted comment on MR !{pr_number} in {owner}/{repo}",
                extra={"repo_id": f"{owner}/{repo}", "mr_iid": pr_number},
            )

        # Handle network timeout errors
        except httpx.TimeoutException:
            logger.error(
                f"Timeout posting comment on MR !{pr_number} in {owner}/{repo}",
                extra={"repo_id": f"{owner}/{repo}", "mr_iid": pr_number},
            )
            raise ValueError(
                f"GitLab API request timed out after {self.client.timeout.read}s "
                f"posting comment on MR !{pr_number} in {owner}/{repo}."
            )
        except (httpx.ConnectError, httpx.ConnectTimeout):
            logger.error(
                f"Failed to connect to GitLab API for {owner}/{repo}",
                extra={"repo_id": f"{owner}/{repo}"},
            )
            raise ValueError(
                f"Cannot connect to GitLab API at {self.api_url} for {owner}/{repo}. "
                "Check your internet connection, firewall, or GitLab API status."
            )
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
                f"Failed to create MR comment on !{pr_number} in {owner}/{repo}: "
                f"{e.response.text}"
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

        Args:
            owner: Project namespace
            repo: Project name
            pr_number: Merge request IID
            file_path: Path to file being commented on (relative to repo root)
            line: Line number in the file (must be part of MR diff)
            body: Comment body (markdown supported)

        Raises:
            ValueError: If review comment creation fails or network/API error occurs

        Note:
            GitLab uses "discussions" for inline comments with position objects.
            Position requires: base_sha, head_sha, start_sha, new_path, new_line.
            These are extracted from the MR's diff_refs.
        """
        # First, get MR data to extract commit SHAs from diff_refs
        mr_data = await self.get_pr(owner, repo, pr_number)

        # Validate required diff_refs field exists
        if "diff_refs" not in mr_data:
            logger.error(
                f"MR !{pr_number} response missing diff_refs in {owner}/{repo}",
                extra={"repo_id": f"{owner}/{repo}", "mr_iid": pr_number, "mr_data": mr_data},
            )
            raise ValueError(
                f"GitLab API response for MR !{pr_number} in {owner}/{repo} "
                "missing required 'diff_refs' field. MR may not have commits yet."
            )

        diff_refs = mr_data["diff_refs"]

        # Validate all required SHAs exist
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

        base_sha = diff_refs["base_sha"]
        head_sha = diff_refs["head_sha"]
        start_sha = diff_refs["start_sha"]

        project_id = self._encode_project_path(owner, repo)
        url = f"{self.api_url}/projects/{project_id}/merge_requests/{pr_number}/discussions"

        # Build position object (GitLab-specific format for inline comments)
        position = {
            "base_sha": base_sha,
            "start_sha": start_sha,
            "head_sha": head_sha,
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
            except json.JSONDecodeError:
                logger.error(
                    f"GitLab API returned non-JSON response for review comment on MR !{pr_number}",
                    extra={"response_text": response.text[:200]},
                )
                raise ValueError(
                    f"GitLab API returned invalid JSON after posting review comment on "
                    f"MR !{pr_number} in {owner}/{repo}: {response.text[:200]}"
                )

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
        except httpx.TimeoutException:
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
            )
        except (httpx.ConnectError, httpx.ConnectTimeout):
            logger.error(
                f"Failed to connect to GitLab API for {owner}/{repo}",
                extra={"repo_id": f"{owner}/{repo}"},
            )
            raise ValueError(
                f"Cannot connect to GitLab API at {self.api_url} for {owner}/{repo}. "
                "Check your internet connection, firewall, or GitLab API status."
            )
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
                )

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
            )

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
        """Post an inline review comment on specific line (Gitea-compatible interface).

        This method provides the same interface as Gitea/GitHub adapters for compatibility
        with PRReviewAnalyzer, which calls adapter.create_pr_review_comment().

        Args:
            owner: Project namespace
            repo: Project name
            pr_number: Merge request IID
            commit_sha: Commit SHA (ignored for GitLab - uses head_sha from diff_refs)
            file_path: File path relative to repo root
            line: Line number in new version
            body: Comment body (markdown supported)

        Raises:
            ValueError: If review comment creation fails

        Note:
            GitLab uses discussions with position objects, not commit-based reviews.
            The commit_sha parameter is accepted for API compatibility but not used.
            Instead, GitLab requires base_sha/head_sha/start_sha from MR diff_refs.
        """
        # Delegate to post_review_comment which implements GitLab-specific logic
        # The commit_sha parameter is ignored as GitLab uses diff_refs instead
        await self.post_review_comment(owner, repo, pr_number, file_path, line, body)

    async def get_file_content(self, owner: str, repo: str, file_path: str, ref: str) -> str:
        """Get file content at a specific git reference.

        Args:
            owner: Project namespace
            repo: Project name
            file_path: Path to file (relative to repo root)
            ref: Git reference (branch name, tag, or commit SHA)

        Returns:
            File content as string

        Raises:
            ValueError: If file not found, not UTF-8 text, or network/API error occurs

        Note:
            GitLab returns content base64-encoded in the API response.
            This method only supports text files (UTF-8). Binary files will raise ValueError.
            File path must also be URL-encoded for GitLab API.
        """
        project_id = self._encode_project_path(owner, repo)
        # File path must also be URL-encoded
        encoded_file_path = urllib.parse.quote(file_path, safe="")

        url = f"{self.api_url}/projects/{project_id}/repository/files/{encoded_file_path}"
        params = {"ref": ref}

        try:
            response = await self.client.get(url, params=params)
            response.raise_for_status()

            # Validate JSON parsing to handle non-JSON error responses
            try:
                data = response.json()
            except json.JSONDecodeError:
                logger.error(
                    f"GitLab API returned non-JSON response for {file_path} in {owner}/{repo}",
                    extra={"response_text": response.text[:200]},
                )
                raise ValueError(
                    f"GitLab API returned invalid JSON for {file_path} in {owner}/{repo}: "
                    f"{response.text[:200]}"
                )

            # GitLab returns base64-encoded content - validate field exists
            if "content" not in data:
                logger.error(
                    f"GitLab API response missing 'content' field for {file_path}",
                    extra={
                        "repo_id": f"{owner}/{repo}",
                        "file_path": file_path,
                        "ref": ref,
                        "response": data,
                    },
                )
                raise ValueError(
                    f"GitLab API response missing 'content' field for {file_path} "
                    f"in {owner}/{repo}@{ref}. API response may be malformed."
                )

            content_b64 = data["content"]

            # Empty file - valid case
            if not content_b64 or content_b64.strip() == "":
                logger.debug(
                    f"Retrieved empty file {file_path} from {owner}/{repo}@{ref}",
                    extra={"repo_id": f"{owner}/{repo}", "file_path": file_path, "ref": ref},
                )
                return ""

            # Handle base64 decode and UTF-8 decode errors
            try:
                # GitLab may include newlines in the base64, so remove them first
                content_b64 = content_b64.replace("\n", "")
                decoded_bytes = base64.b64decode(content_b64)
                decoded_str = decoded_bytes.decode("utf-8")

                logger.debug(
                    f"Retrieved file {file_path} from {owner}/{repo}@{ref}",
                    extra={
                        "repo_id": f"{owner}/{repo}",
                        "file_path": file_path,
                        "ref": ref,
                        "size": len(decoded_str),
                    },
                )

                return decoded_str

            except UnicodeDecodeError:
                logger.error(
                    f"File {file_path} in {owner}/{repo}@{ref} contains non-UTF8 content",
                    extra={"repo_id": f"{owner}/{repo}", "file_path": file_path, "ref": ref},
                )
                raise ValueError(
                    f"File {file_path} in {owner}/{repo}@{ref} is binary or non-UTF8. "
                    "GitLab adapter only supports text files."
                )
            except (base64.binascii.Error, ValueError):
                logger.error(
                    f"Failed to decode base64 for {file_path} in {owner}/{repo}@{ref}",
                    extra={"repo_id": f"{owner}/{repo}", "file_path": file_path, "ref": ref},
                )
                raise ValueError(
                    f"Failed to decode file content (invalid base64) for {file_path} "
                    f"in {owner}/{repo}@{ref}. File may be corrupted."
                )

        # Handle network timeout errors
        except httpx.TimeoutException:
            logger.error(
                f"Timeout fetching {file_path} from {owner}/{repo}@{ref}",
                extra={"repo_id": f"{owner}/{repo}", "file_path": file_path, "ref": ref},
            )
            raise ValueError(
                f"GitLab API request timed out after {self.client.timeout.read}s "
                f"fetching {file_path} from {owner}/{repo}@{ref}. File may be very large."
            )
        except (httpx.ConnectError, httpx.ConnectTimeout):
            logger.error(
                f"Failed to connect to GitLab API for {owner}/{repo}",
                extra={"repo_id": f"{owner}/{repo}"},
            )
            raise ValueError(
                f"Cannot connect to GitLab API at {self.api_url} for {owner}/{repo}. "
                "Check your internet connection, firewall, or GitLab API status."
            )
        except httpx.HTTPStatusError as e:
            if e.response.status_code == 404:
                logger.warning(
                    f"File {file_path} not found in {owner}/{repo}@{ref}",
                    extra={"repo_id": f"{owner}/{repo}", "file_path": file_path, "ref": ref},
                )
                raise ValueError(f"File {file_path} not found at ref {ref} in {owner}/{repo}")
            else:
                # Check for rate limit exceeded
                self._check_rate_limit(e.response, owner, repo)

                logger.error(
                    f"HTTP error fetching {file_path} from {owner}/{repo}@{ref}: "
                    f"{e.response.status_code}",
                    extra={
                        "repo_id": f"{owner}/{repo}",
                        "file_path": file_path,
                        "ref": ref,
                        "http_status": e.response.status_code,
                        "response_text": e.response.text,
                    },
                )
                raise ValueError(
                    f"Failed to fetch {file_path} from {owner}/{repo}@{ref}: {e.response.text}"
                )
