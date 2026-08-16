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
import binascii
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
        except httpx.TimeoutException as exc:
            logger.error(
                f"Timeout fetching default branch for {owner}/{repo}",
                extra={"repo_id": f"{owner}/{repo}"},
            )
            raise ValueError(
                f"GitLab API request timed out after {self.client.timeout.read}s "
                f"fetching default branch for {owner}/{repo}. "
                "Project may be very large, or GitLab API is slow."
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
        except httpx.TimeoutException as exc:
            logger.error(
                f"Timeout creating issue in {owner}/{repo}",
                extra={"repo_id": f"{owner}/{repo}", "timeout": self.client.timeout.read},
            )
            raise ValueError(
                f"GitLab API request timed out after {self.client.timeout.read}s "
                f"for {owner}/{repo}. GitLab API may be slow or project may be large."
            ) from exc
        except (httpx.ConnectError, httpx.ConnectTimeout) as exc:
            logger.error(
                f"Failed to connect to GitLab API for {owner}/{repo}",
                extra={"repo_id": f"{owner}/{repo}", "api_url": self.api_url},
            )
            raise ValueError(
                f"Cannot connect to GitLab API at {self.api_url} for {owner}/{repo}. "
                "Check your internet connection, firewall, or GitLab API status."
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

            except UnicodeDecodeError as exc:
                logger.error(
                    f"File {file_path} in {owner}/{repo}@{ref} contains non-UTF8 content",
                    extra={"repo_id": f"{owner}/{repo}", "file_path": file_path, "ref": ref},
                )
                raise ValueError(
                    f"File {file_path} in {owner}/{repo}@{ref} is binary or non-UTF8. "
                    "GitLab adapter only supports text files."
                ) from exc
            except (binascii.Error, ValueError) as exc:
                logger.error(
                    f"Failed to decode base64 for {file_path} in {owner}/{repo}@{ref}",
                    extra={"repo_id": f"{owner}/{repo}", "file_path": file_path, "ref": ref},
                )
                raise ValueError(
                    f"Failed to decode file content (invalid base64) for {file_path} "
                    f"in {owner}/{repo}@{ref}. File may be corrupted."
                ) from exc

        # Handle network timeout errors
        except httpx.TimeoutException as exc:
            logger.error(
                f"Timeout fetching {file_path} from {owner}/{repo}@{ref}",
                extra={"repo_id": f"{owner}/{repo}", "file_path": file_path, "ref": ref},
            )
            raise ValueError(
                f"GitLab API request timed out after {self.client.timeout.read}s "
                f"fetching {file_path} from {owner}/{repo}@{ref}. File may be very large."
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
