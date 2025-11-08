"""GitHub platform adapter implementation."""

import asyncio
import base64
import json
import logging
from typing import Dict, List, Optional

import httpx

from drep.adapters.base import BaseAdapter

logger = logging.getLogger(__name__)


class GitHubAdapter(BaseAdapter):
    """GitHub API adapter.

    Uses GitHub REST API v3 for all operations. GitHub has different API
    patterns compared to Gitea:

    Authentication:
        - GitHub: 'Authorization: Bearer {token}' header
        - Gitea: 'Authorization: token {token}' header

    Labels:
        - GitHub: Use label names directly (strings)
        - Gitea: Translate names to integer IDs via label cache

    Review Comments:
        - GitHub: Uses 'line' + 'side' (LEFT/RIGHT) fields
        - Gitea: Uses 'new_position' or 'position' fields (multi-version support)

    PR Diff Format:
        - GitHub: Accept header negotiation (application/vnd.github.v3.diff)
        - Gitea: Direct diff endpoint

    Limitations:
        - get_file_content() only supports UTF-8 text files
        - Binary files will raise ValueError
        - Review comments only support added lines (side="RIGHT"), not deleted lines

    Design Notes:
        - Constructor parameter order (token, url) differs from GiteaAdapter (url, token)
        - GitHub.com default URL makes token-first more ergonomic for common case
        - Consider standardizing across adapters in future if this causes confusion
    """

    def __init__(self, token: str, url: str = "https://api.github.com"):
        """Initialize GitHubAdapter with token.

        Args:
            token: GitHub Personal Access Token (PAT) or GitHub App token as plain string.
                   IMPORTANT: If loading from GitHubConfig (Pydantic model), you must
                   unwrap SecretStr by calling: config.github.token.get_secret_value()
            url: GitHub API base URL (default: https://api.github.com) as plain string.
                 IMPORTANT: If loading from GitHubConfig (Pydantic model), you must
                 convert HttpUrl to str by calling: str(config.github.url)

        Raises:
            ValueError: If token is empty or URL is invalid

        Example:
            # Direct usage with plain strings
            adapter = GitHubAdapter(token="ghp_...")
            try:
                issue_num = await adapter.create_issue("owner", "repo", "Title", "Body")
            finally:
                await adapter.close()

            # GitHub Enterprise
            adapter = GitHubAdapter(
                token="ghp_...",
                url="https://github.company.com/api/v3"
            )

            # Loading from GitHubConfig (CRITICAL: unwrap SecretStr and convert HttpUrl)
            from drep.config import load_config

            config = load_config("config.yaml")
            if config.github:
                adapter = GitHubAdapter(
                    token=config.github.token.get_secret_value(),  # Unwrap SecretStr!
                    url=str(config.github.url)  # Convert HttpUrl to str!
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
            raise ValueError("GitHub token cannot be empty")

        # Validate URL starts with http:// or https://
        if not url.startswith(("http://", "https://")):
            raise ValueError(f"GitHub URL must start with http:// or https://, got: {url}")

        self.url = url.rstrip("/")
        self.token = token.strip()
        self.client = httpx.AsyncClient(
            headers={
                "Authorization": f"Bearer {self.token}",
                "Accept": "application/vnd.github.v3+json",
            },
            timeout=30.0,
        )

        logger.debug("Initialized GitHub adapter", extra={"api_url": self.url, "timeout": 30.0})

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
            logger.debug("Closed GitHub adapter HTTP client")
        except (KeyboardInterrupt, SystemExit, asyncio.CancelledError):
            # Always propagate user interrupts, system exit signals, and async cancellations
            logger.info("Close interrupted by user or system")
            raise
        except Exception as e:
            # Suppress cleanup errors to avoid masking original errors in finally blocks
            # (httpx.CloseError, RuntimeError, etc.)
            logger.warning(f"Non-critical error closing GitHub client: {e}")

    def _check_rate_limit(self, response: httpx.Response, owner: str = "", repo: str = "") -> None:
        """Check for rate limit and raise informative error.

        Args:
            response: HTTP response from GitHub API
            owner: Repository owner (for error context)
            repo: Repository name (for error context)

        Raises:
            ValueError: If rate limit is exceeded

        Note:
            GitHub returns rate limit info in these headers:
            - X-RateLimit-Limit: Maximum requests per hour
            - X-RateLimit-Remaining: Requests remaining (string or int)
            - X-RateLimit-Reset: Unix timestamp when limit resets
        """
        if response.status_code != 403:
            return  # Not a rate limit error

        remaining_str = response.headers.get("X-RateLimit-Remaining")
        if remaining_str is None:
            return  # Not a rate limit response

        # Parse remaining count robustly (handle "0", " 0 ", "0.0", etc.)
        try:
            remaining = int(float(remaining_str.strip()))
        except (ValueError, TypeError):
            # Can't parse - not a valid rate limit header
            return

        if remaining == 0:
            reset_time = response.headers.get("X-RateLimit-Reset", "unknown")
            context = f" for {owner}/{repo}" if owner and repo else ""
            repo_id = f"{owner}/{repo}" if owner and repo else None
            logger.warning(
                f"GitHub API rate limit exceeded{context}",
                extra={"repo_id": repo_id, "reset_time": reset_time},
            )
            raise ValueError(
                f"GitHub API rate limit exceeded. Resets at {reset_time}. "
                "Wait or use a different token."
            )

    # ===== Repository Methods =====

    async def get_default_branch(self, owner: str, repo: str) -> str:
        """Get repository default branch name.

        Args:
            owner: Repository owner
            repo: Repository name

        Returns:
            Default branch name (e.g., "main", "master", "develop")

        Raises:
            ValueError: If repository not found (404) or network/API error occurs

        Example:
            branch = await adapter.get_default_branch("owner", "repo")
            # Returns: "main"
        """
        url = f"{self.url}/repos/{owner}/{repo}"

        try:
            response = await self.client.get(url)
            response.raise_for_status()

            # Validate JSON parsing to handle non-JSON error responses
            try:
                data = response.json()
            except json.JSONDecodeError:
                logger.error(
                    f"GitHub API returned non-JSON response for {owner}/{repo}",
                    extra={"response_text": response.text[:200]},
                )
                raise ValueError(
                    f"GitHub API returned invalid JSON for {owner}/{repo}: "
                    f"{response.text[:200]}"
                )

            # Validate required 'default_branch' field exists in API response
            if "default_branch" not in data:
                logger.error(
                    f"GitHub response missing 'default_branch' field for {owner}/{repo}",
                    extra={"response": data},
                )
                raise ValueError(
                    f"GitHub API response missing 'default_branch' field for {owner}/{repo}"
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
                f"GitHub API request timed out after {self.client.timeout.read}s "
                f"fetching default branch for {owner}/{repo}. "
                "Repository may be very large, or GitHub API is slow."
            )
        except (httpx.ConnectError, httpx.ConnectTimeout):
            logger.error(
                f"Failed to connect to GitHub API for {owner}/{repo}",
                extra={"repo_id": f"{owner}/{repo}"},
            )
            raise ValueError(
                f"Cannot connect to GitHub API at {self.url} for {owner}/{repo}. "
                "Check your internet connection, firewall, or GitHub API status."
            )
        except httpx.HTTPStatusError as e:
            if e.response.status_code == 404:
                logger.warning(
                    f"Repository {owner}/{repo} not found",
                    extra={"repo_id": f"{owner}/{repo}"},
                )
                raise ValueError(f"Repository {owner}/{repo} not found")
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
                    f"GitHub API error fetching default branch for {owner}/{repo}: "
                    f"{e.response.text}"
                )

    # ===== Issue Methods =====

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
            ValueError: If issue creation fails due to:
                - Network timeout (request exceeds 30s timeout)
                - Connection failure (cannot reach GitHub API)
                - GitHub API rate limit exceeded (X-RateLimit-Remaining: 0)
                - Invalid JSON response from GitHub API
                - Missing required 'number' field in API response
                - HTTP errors (401 Unauthorized, 403 Forbidden, 500 Server Error, etc.)
        """
        url = f"{self.url}/repos/{owner}/{repo}/issues"
        payload = {"title": title, "body": body}

        # GitHub uses label names directly (not IDs like Gitea)
        if labels:
            payload["labels"] = labels

        try:
            response = await self.client.post(url, json=payload)
            response.raise_for_status()

            # Validate JSON parsing to handle non-JSON error responses
            try:
                data = response.json()
            except json.JSONDecodeError:
                logger.error(
                    f"GitHub API returned non-JSON response for {owner}/{repo}",
                    extra={"response_text": response.text[:200]},
                )
                raise ValueError(
                    f"GitHub API returned invalid JSON for {owner}/{repo}: "
                    f"{response.text[:200]}"
                )

            # Validate required 'number' field exists in API response
            if "number" not in data:
                logger.error(
                    f"GitHub response missing 'number' field for {owner}/{repo}",
                    extra={"response": data},
                )
                raise ValueError(f"GitHub API response missing 'number' field for {owner}/{repo}")

            issue_number = data["number"]

            # Log successful issue creation with context
            logger.debug(
                f"Created issue #{issue_number} in {owner}/{repo}",
                extra={"repo_id": f"{owner}/{repo}", "issue_number": issue_number},
            )

            return issue_number

        # Handle network timeout errors
        except httpx.TimeoutException:
            logger.error(
                f"Timeout creating issue in {owner}/{repo}",
                extra={"repo_id": f"{owner}/{repo}", "timeout": self.client.timeout.read},
            )
            raise ValueError(
                f"GitHub API request timed out after {self.client.timeout.read}s "
                f"for {owner}/{repo}. GitHub API may be slow or repository may be large."
            )
        except (httpx.ConnectError, httpx.ConnectTimeout):
            logger.error(
                f"Failed to connect to GitHub API for {owner}/{repo}",
                extra={"repo_id": f"{owner}/{repo}", "api_url": self.url},
            )
            raise ValueError(
                f"Cannot connect to GitHub API at {self.url} for {owner}/{repo}. "
                "Check your internet connection, firewall, or GitHub API status."
            )
        except httpx.HTTPStatusError as e:
            # Check for rate limit exceeded
            self._check_rate_limit(e.response, owner, repo)

            # Include repository context in error message for debugging
            logger.error(
                f"HTTP error creating issue in {owner}/{repo}: {e.response.status_code}",
                extra={
                    "repo_id": f"{owner}/{repo}",
                    "http_status": e.response.status_code,
                    "response_text": e.response.text,
                },
            )
            raise ValueError(f"Failed to create issue in {owner}/{repo}: {e.response.text}")

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
            ValueError: If PR not found (404) or network/API error occurs
        """
        url = f"{self.url}/repos/{owner}/{repo}/pulls/{pr_number}"

        try:
            response = await self.client.get(url)
            response.raise_for_status()

            # Validate JSON parsing to handle non-JSON error responses
            try:
                data = response.json()
            except json.JSONDecodeError:
                logger.error(
                    f"GitHub API returned non-JSON response for PR #{pr_number} in {owner}/{repo}",
                    extra={"response_text": response.text[:200]},
                )
                raise ValueError(
                    f"GitHub API returned invalid JSON for {owner}/{repo} PR #{pr_number}: "
                    f"{response.text[:200]}"
                )

            logger.debug(
                f"Retrieved PR #{pr_number} from {owner}/{repo}",
                extra={"repo_id": f"{owner}/{repo}", "pr_number": pr_number},
            )

            return data

        # Handle network timeout errors
        except httpx.TimeoutException:
            logger.error(
                f"Timeout fetching PR #{pr_number} from {owner}/{repo}",
                extra={"repo_id": f"{owner}/{repo}", "pr_number": pr_number},
            )
            raise ValueError(
                f"GitHub API request timed out after {self.client.timeout.read}s "
                f"fetching PR #{pr_number} from {owner}/{repo}. "
                "PR may be very large, or GitHub API is slow."
            )
        except (httpx.ConnectError, httpx.ConnectTimeout):
            logger.error(
                f"Failed to connect to GitHub API for {owner}/{repo}",
                extra={"repo_id": f"{owner}/{repo}"},
            )
            raise ValueError(
                f"Cannot connect to GitHub API at {self.url} for {owner}/{repo}. "
                "Check your internet connection, firewall, or GitHub API status."
            )
        except httpx.HTTPStatusError as e:
            if e.response.status_code == 404:
                logger.warning(
                    f"Pull request #{pr_number} not found in {owner}/{repo}",
                    extra={"repo_id": f"{owner}/{repo}", "pr_number": pr_number},
                )
                raise ValueError(f"Pull request #{pr_number} not found in {owner}/{repo}")
            else:
                # Check for rate limit exceeded
                self._check_rate_limit(e.response, owner, repo)

                logger.error(
                    f"HTTP error fetching PR #{pr_number} from {owner}/{repo}: "
                    f"{e.response.status_code}",
                    extra={
                        "repo_id": f"{owner}/{repo}",
                        "pr_number": pr_number,
                        "http_status": e.response.status_code,
                        "response_text": e.response.text,
                    },
                )
                raise ValueError(
                    f"GitHub API error fetching PR #{pr_number} from {owner}/{repo}: "
                    f"{e.response.text}"
                )

    async def get_pr_diff(self, owner: str, repo: str, pr_number: int) -> str:
        """Get pull request diff in unified diff format.

        Args:
            owner: Repository owner
            repo: Repository name
            pr_number: Pull request number

        Returns:
            Unified diff string (can be very large)

        Raises:
            ValueError: If network/API error occurs

        Note:
            GitHub provides diff via Accept header on the PR endpoint.
            Using Accept: application/vnd.github.v3.diff returns diff directly.
        """
        url = f"{self.url}/repos/{owner}/{repo}/pulls/{pr_number}"

        try:
            # Request diff format using GitHub's media type negotiation
            response = await self.client.get(
                url, headers={"Accept": "application/vnd.github.v3.diff"}
            )
            response.raise_for_status()

            logger.debug(
                f"Retrieved diff for PR #{pr_number} from {owner}/{repo}",
                extra={
                    "repo_id": f"{owner}/{repo}",
                    "pr_number": pr_number,
                    "diff_size": len(response.text),
                },
            )

            return response.text

        # Handle network timeout errors
        except httpx.TimeoutException:
            logger.error(
                f"Timeout fetching PR diff for #{pr_number} from {owner}/{repo}",
                extra={"repo_id": f"{owner}/{repo}", "pr_number": pr_number},
            )
            raise ValueError(
                f"GitHub API request timed out after {self.client.timeout.read}s "
                f"fetching PR #{pr_number} diff from {owner}/{repo}. PR diff may be very large."
            )
        except (httpx.ConnectError, httpx.ConnectTimeout):
            logger.error(
                f"Failed to connect to GitHub API for {owner}/{repo}",
                extra={"repo_id": f"{owner}/{repo}"},
            )
            raise ValueError(
                f"Cannot connect to GitHub API at {self.url} for {owner}/{repo}. "
                "Check your internet connection, firewall, or GitHub API status."
            )
        except httpx.HTTPStatusError as e:
            # Check for rate limit exceeded
            self._check_rate_limit(e.response, owner, repo)

            logger.error(
                f"HTTP error fetching PR diff for #{pr_number} from {owner}/{repo}: "
                f"{e.response.status_code}",
                extra={
                    "repo_id": f"{owner}/{repo}",
                    "pr_number": pr_number,
                    "http_status": e.response.status_code,
                },
            )
            raise ValueError(
                f"Failed to fetch PR #{pr_number} diff from {owner}/{repo}: {e.response.text}"
            )

    async def create_pr_comment(self, owner: str, repo: str, pr_number: int, body: str) -> None:
        """Post a general comment on the PR (not line-specific).

        Args:
            owner: Repository owner
            repo: Repository name
            pr_number: Pull request number
            body: Comment body (markdown supported)

        Raises:
            ValueError: If comment creation fails or network/API error occurs

        Note:
            GitHub uses the issues API for PR comments (PRs are special issues).
        """
        url = f"{self.url}/repos/{owner}/{repo}/issues/{pr_number}/comments"
        payload = {"body": body}

        try:
            response = await self.client.post(url, json=payload)
            response.raise_for_status()

            logger.debug(
                f"Posted comment on PR #{pr_number} in {owner}/{repo}",
                extra={"repo_id": f"{owner}/{repo}", "pr_number": pr_number},
            )

        # Handle network timeout errors
        except httpx.TimeoutException:
            logger.error(
                f"Timeout posting comment on PR #{pr_number} in {owner}/{repo}",
                extra={"repo_id": f"{owner}/{repo}", "pr_number": pr_number},
            )
            raise ValueError(
                f"GitHub API request timed out after {self.client.timeout.read}s "
                f"posting comment on PR #{pr_number} in {owner}/{repo}."
            )
        except (httpx.ConnectError, httpx.ConnectTimeout):
            logger.error(
                f"Failed to connect to GitHub API for {owner}/{repo}",
                extra={"repo_id": f"{owner}/{repo}"},
            )
            raise ValueError(
                f"Cannot connect to GitHub API at {self.url} for {owner}/{repo}. "
                "Check your internet connection, firewall, or GitHub API status."
            )
        except httpx.HTTPStatusError as e:
            # Check for rate limit exceeded
            self._check_rate_limit(e.response, owner, repo)

            logger.error(
                f"HTTP error posting comment on PR #{pr_number} in {owner}/{repo}: "
                f"{e.response.status_code}",
                extra={
                    "repo_id": f"{owner}/{repo}",
                    "pr_number": pr_number,
                    "http_status": e.response.status_code,
                    "response_text": e.response.text,
                },
            )
            raise ValueError(
                f"Failed to create PR comment on #{pr_number} in {owner}/{repo}: "
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
        """Post a line-specific review comment on a PR (BaseAdapter interface).

        Args:
            owner: Repository owner
            repo: Repository name
            pr_number: Pull request number
            file_path: Path to file being commented on (relative to repo root)
            line: Line number in the file (must be part of PR diff)
            body: Comment body (markdown supported)

        Raises:
            ValueError: If review comment creation fails or network/API error occurs

        Note:
            GitHub requires commit_id, line, side, and path fields.
            This method fetches the PR head SHA automatically.

            Implementation currently only supports comments on added/modified lines
            (side="RIGHT"). Comments on deleted lines are not supported. This is
            consistent with drep's current usage pattern of only commenting on
            added code.
        """
        # Get PR details to extract commit SHA
        pr = await self.get_pr(owner, repo, pr_number)

        # Validate required head.sha field exists in PR response
        if "head" not in pr or "sha" not in pr["head"]:
            logger.error(
                f"PR #{pr_number} response missing head.sha in {owner}/{repo}",
                extra={"repo_id": f"{owner}/{repo}", "pr_number": pr_number, "pr_data": pr},
            )
            raise ValueError(
                f"GitHub API response for PR #{pr_number} in {owner}/{repo} "
                "missing required 'head.sha' field"
            )

        commit_sha = pr["head"]["sha"]

        # Post review comment using GitHub's review comments API
        url = f"{self.url}/repos/{owner}/{repo}/pulls/{pr_number}/comments"

        # GitHub requires these fields:
        # - commit_id: SHA of the commit to comment on
        # - path: file path
        # - line: line number
        # - side: "LEFT" (deleted) or "RIGHT" (added)
        # Assumption: Only support comments on added lines (side="RIGHT"), not deleted lines
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
        except httpx.TimeoutException:
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
            )
        except (httpx.ConnectError, httpx.ConnectTimeout):
            logger.error(
                f"Failed to connect to GitHub API for {owner}/{repo}",
                extra={"repo_id": f"{owner}/{repo}"},
            )
            raise ValueError(
                f"Cannot connect to GitHub API at {self.url} for {owner}/{repo}. "
                "Check your internet connection, firewall, or GitHub API status."
            )
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
                )

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

        This method provides a Gitea-compatible interface that accepts commit_sha
        explicitly, rather than fetching it from the PR. This is used by PR analyzers
        that already have the commit SHA available.

        Args:
            owner: Repository owner
            repo: Repository name
            pr_number: Pull request number
            commit_sha: Commit SHA to comment on (usually PR head)
            file_path: File path relative to repo root
            line: Line number in new version (after changes)
            body: Comment body (markdown supported)

        Raises:
            ValueError: If review comment creation fails or network/API error occurs

        Note:
            Implementation currently only supports comments on added/modified lines
            (side="RIGHT"). Comments on deleted lines are not supported.
        """
        # Post review comment using GitHub's review comments API
        url = f"{self.url}/repos/{owner}/{repo}/pulls/{pr_number}/comments"

        # GitHub requires these fields:
        # - commit_id: SHA of the commit to comment on
        # - path: file path
        # - line: line number
        # - side: "LEFT" (deleted) or "RIGHT" (added)
        # Assumption: Only support comments on added lines (side="RIGHT"), not deleted lines
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
        except httpx.TimeoutException:
            logger.error(
                f"Timeout posting review comment on PR #{pr_number} in {owner}/{repo}",
                extra={"repo_id": f"{owner}/{repo}", "pr_number": pr_number},
            )
            raise ValueError(
                f"GitHub API request timed out posting review comment on PR #{pr_number} "
                f"in {owner}/{repo}"
            )
        except (httpx.ConnectError, httpx.ConnectTimeout):
            logger.error(
                f"Failed to connect to GitHub API for {owner}/{repo}",
                extra={"repo_id": f"{owner}/{repo}"},
            )
            raise ValueError(f"Cannot connect to GitHub API at {self.url} for {owner}/{repo}")
        except httpx.HTTPStatusError as e:
            # Check for rate limit exceeded
            self._check_rate_limit(e.response, owner, repo)

            logger.error(
                f"HTTP error posting review comment on PR #{pr_number} in {owner}/{repo}",
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
            )

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
            ValueError: If file not found, not UTF-8 text, or network/API error occurs

        Note:
            GitHub returns content base64-encoded in the API response.
            This method only supports text files (UTF-8). Binary files will raise ValueError.
        """
        url = f"{self.url}/repos/{owner}/{repo}/contents/{file_path}"
        params = {"ref": ref}

        try:
            response = await self.client.get(url, params=params)
            response.raise_for_status()

            # Validate JSON parsing to handle non-JSON error responses
            try:
                data = response.json()
            except json.JSONDecodeError:
                logger.error(
                    f"GitHub API returned non-JSON response for {file_path} in {owner}/{repo}",
                    extra={"response_text": response.text[:200]},
                )
                raise ValueError(
                    f"GitHub API returned invalid JSON for {file_path} in {owner}/{repo}: "
                    f"{response.text[:200]}"
                )

            # GitHub returns base64-encoded content - validate field exists
            if "content" not in data:
                logger.error(
                    f"GitHub API response missing 'content' field for {file_path}",
                    extra={
                        "repo_id": f"{owner}/{repo}",
                        "file_path": file_path,
                        "ref": ref,
                        "response": data,
                    },
                )
                raise ValueError(
                    f"GitHub API response missing 'content' field for {file_path} "
                    f"in {owner}/{repo}@{ref}. API response may be malformed."
                )

            content = data["content"]

            # Empty file - valid case
            if not content or content.strip() == "":
                logger.debug(
                    f"Retrieved empty file {file_path} from {owner}/{repo}@{ref}",
                    extra={"repo_id": f"{owner}/{repo}", "file_path": file_path, "ref": ref},
                )
                return ""

            # Handle base64 decode and UTF-8 decode errors
            try:
                # GitHub may include newlines in the base64, so remove them first
                content = content.replace("\n", "")
                decoded_bytes = base64.b64decode(content)
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
                    "GitHub adapter only supports text files."
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
                f"GitHub API request timed out after {self.client.timeout.read}s "
                f"fetching {file_path} from {owner}/{repo}@{ref}. File may be very large."
            )
        except (httpx.ConnectError, httpx.ConnectTimeout):
            logger.error(
                f"Failed to connect to GitHub API for {owner}/{repo}",
                extra={"repo_id": f"{owner}/{repo}"},
            )
            raise ValueError(
                f"Cannot connect to GitHub API at {self.url} for {owner}/{repo}. "
                "Check your internet connection, firewall, or GitHub API status."
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
