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

import json
import logging
import urllib.parse
from datetime import datetime

import httpx

from drep.adapters.base import BaseAdapter
from drep.adapters.gitlab_prs import GitLabPrMixin
from drep.adapters.gitlab_reviews import GitLabReviewMixin

logger = logging.getLogger(__name__)


class GitLabAdapter(GitLabPrMixin, GitLabReviewMixin, BaseAdapter):
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

    # platform_name / api_base_url / _encode_project_path come from GitLabMixinBase

    def __init__(self, token: str, url: str | None = None):
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
            clean_url = clean_url.removesuffix("/api/v4")  # Remove "/api/v4"
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

    def git_clone_url(self, owner: str, repo: str) -> str:
        """Return the HTTPS git clone URL for a GitLab project."""
        return f"{self.base_url.rstrip('/')}/{owner}/{repo}.git"

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
            except json.JSONDecodeError as exc:
                logger.error(
                    f"GitLab API returned non-JSON response for {owner}/{repo}",
                    extra={"response_text": response.text[:200]},
                )
                raise ValueError(
                    f"GitLab API returned invalid JSON for {owner}/{repo}: {response.text[:200]}"
                ) from exc

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
        except self.NETWORK_ERRORS as exc:
            raise self._network_error(
                exc,
                f"fetching default branch for {owner}/{repo}",
                f"{owner}/{repo}",
                "Project may be very large, or GitLab API is slow.",
            ) from exc
        except httpx.HTTPStatusError as e:
            if e.response.status_code == 404:
                logger.warning(
                    f"Project {owner}/{repo} not found",
                    extra={"repo_id": f"{owner}/{repo}"},
                )
                raise ValueError(f"GitLab project {owner}/{repo} not found") from e
            # Check for rate limit exceeded
            self._check_rate_limit(e.response, owner, repo)

            logger.error(
                f"HTTP error fetching default branch for {owner}/{repo}: {e.response.status_code}",
                extra={
                    "repo_id": f"{owner}/{repo}",
                    "http_status": e.response.status_code,
                    "response_text": e.response.text,
                },
            )
            raise ValueError(
                f"GitLab API error fetching default branch for {owner}/{repo}: {e.response.text}"
            ) from e

    # ===== Issue Methods =====

    async def create_issue(
        self, owner: str, repo: str, title: str, body: str, labels: list[str] | None = None
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
            except json.JSONDecodeError as exc:
                logger.error(
                    f"GitLab API returned non-JSON response for {owner}/{repo}",
                    extra={"response_text": response.text[:200]},
                )
                raise ValueError(
                    f"GitLab API returned invalid JSON for {owner}/{repo}: {response.text[:200]}"
                ) from exc

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
        except self.NETWORK_ERRORS as exc:
            raise self._network_error(
                exc,
                f"for {owner}/{repo}",
                f"{owner}/{repo}",
                "GitLab API may be slow or project may be large.",
            ) from exc
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
            raise ValueError(f"Failed to create issue in {owner}/{repo}: {e.response.text}") from e

    # ===== MR Review Methods =====

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
            except json.JSONDecodeError as exc:
                logger.error(
                    f"GitLab API returned non-JSON response for {file_path} in {owner}/{repo}",
                    extra={"response_text": response.text[:200]},
                )
                raise ValueError(
                    f"GitLab API returned invalid JSON for {file_path} in {owner}/{repo}: "
                    f"{response.text[:200]}"
                ) from exc

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

            return self._decode_file_content(content_b64, owner, repo, file_path, ref)

        # Handle network timeout errors
        except self.NETWORK_ERRORS as exc:
            raise self._network_error(
                exc,
                f"fetching {file_path} from {owner}/{repo}@{ref}",
                f"{owner}/{repo}",
                "File may be very large.",
            ) from exc
        except httpx.HTTPStatusError as e:
            if e.response.status_code == 404:
                logger.warning(
                    f"File {file_path} not found in {owner}/{repo}@{ref}",
                    extra={"repo_id": f"{owner}/{repo}", "file_path": file_path, "ref": ref},
                )
                raise ValueError(
                    f"File {file_path} not found at ref {ref} in {owner}/{repo}"
                ) from e
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
            ) from e
