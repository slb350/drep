"""Abstract base class for platform adapters (Gitea, GitHub, GitLab).

This module defines the common interface that all platform adapters must implement.
By using an abstract base class, we ensure:
- Compile-time verification of interface compliance
- Consistent API across all platforms
- Better IDE autocomplete and type checking
- Easier to add new platform adapters
"""

import asyncio
import base64
import binascii
import logging
from abc import ABC, abstractmethod
from dataclasses import dataclass
from typing import ClassVar

import httpx

logger = logging.getLogger(__name__)


@dataclass(frozen=True)
class ReviewAnchor:
    """Immutable positioning data for inline review comments.

    Fetched once per review via ``get_review_anchor`` and reused for every
    inline comment, so a review batch is anchored to a single consistent
    snapshot of the PR/MR (no per-comment refetching, no mid-loop drift).

    Attributes:
        owner: Repository owner
        repo: Repository name
        pr_number: Pull request / merge request number
        commit_sha: Head commit SHA the comments anchor to
    """

    owner: str
    repo: str
    pr_number: int
    commit_sha: str


class BaseAdapter(ABC):
    """Abstract base class for git platform adapters.

    All platform adapters (Gitea, GitHub, GitLab, etc.) must inherit from this
    class and implement all abstract methods. This ensures a consistent interface
    for interacting with different git hosting platforms.

    The adapter pattern allows drep to work with multiple platforms without
    tying the core logic to any specific platform's API.
    """

    #: Human-readable platform name used in user-facing error messages.
    platform_name: ClassVar[str] = "Platform"

    #: Set by each concrete adapter in __init__.
    client: httpx.AsyncClient

    @property
    def api_base_url(self) -> str:
        """Base URL reported in connection-failure messages."""
        raise NotImplementedError

    #: Transport failures every endpoint translates identically. Catch this tuple
    #: and delegate to _network_error(); HTTP status errors stay at the call site,
    #: since their handling is endpoint-specific.
    NETWORK_ERRORS: ClassVar[tuple[type[Exception], ...]] = (
        httpx.TimeoutException,
        httpx.ConnectError,
        httpx.ConnectTimeout,
    )

    def _check_rate_limit(  # noqa: B027 - intentional no-op default, not an abstract hook
        self, response: httpx.Response, owner: str = "", repo: str = ""
    ) -> None:
        """Raise an informative error if the response signals rate limiting.

        No-op by default: platforms that expose rate-limit headers (GitHub,
        GitLab) override this with their own status/header policy, which differs
        enough between them that a single parameterized body would obscure more
        than it shares. Declaring it here keeps the hook visible to every
        adapter and mixin without per-file typing stubs.

        Args:
            response: HTTP response to inspect
            owner: Repository owner (for error context)
            repo: Repository name (for error context)
        """

    def _decode_file_content(
        self, content_b64: str, owner: str, repo: str, file_path: str, ref: str
    ) -> str:
        """Decode a base64 file payload returned by a platform's contents API.

        GitHub and GitLab both return base64 with embedded newlines and both
        need the same binary/corrupt-content diagnostics, so the decode lives
        here rather than being copied into each adapter.

        Args:
            content_b64: Base64 payload from the API (may contain newlines)
            owner: Repository owner (for error context)
            repo: Repository name (for error context)
            file_path: File path (for error context)
            ref: Git ref (for error context)

        Returns:
            Decoded UTF-8 text

        Raises:
            ValueError: If the content is not valid base64 or not UTF-8 text
        """
        location = f"{file_path} in {owner}/{repo}@{ref}"
        context = {"repo_id": f"{owner}/{repo}", "file_path": file_path, "ref": ref}

        try:
            decoded_str = base64.b64decode(content_b64.replace("\n", "")).decode("utf-8")
        except UnicodeDecodeError as exc:
            logger.error(f"File {location} contains non-UTF8 content", extra=context)
            raise ValueError(
                f"File {location} is binary or non-UTF8. "
                f"{self.platform_name} adapter only supports text files."
            ) from exc
        except (binascii.Error, ValueError) as exc:
            logger.error(f"Failed to decode base64 for {location}", extra=context)
            raise ValueError(
                f"Failed to decode file content (invalid base64) for {location}. "
                "File may be corrupted."
            ) from exc

        logger.debug(
            f"Retrieved file {file_path} from {owner}/{repo}@{ref}",
            extra={**context, "size": len(decoded_str)},
        )
        return decoded_str

    def _network_error(
        self, exc: Exception, operation: str, target: str, hint: str = ""
    ) -> ValueError:
        """Build the ValueError for an httpx transport failure.

        Every endpoint reports timeouts and connection failures with the same
        wording; only the operation noun differs. Usage:

        ::

            except self.NETWORK_ERRORS as exc:
                raise self._network_error(exc, f"fetching MR !{pr_number}", repo_id) from exc

        Args:
            exc: The httpx transport exception
            operation: Present-participle phrase, e.g. "fetching MR !42"
            target: What is being operated on, usually "owner/repo"
            hint: Optional extra sentence appended to the timeout message

        Returns:
            ValueError with a consistent, user-actionable message
        """
        if isinstance(exc, httpx.TimeoutException) and not isinstance(exc, httpx.ConnectTimeout):
            logger.error(f"Timeout {operation}", extra={"repo_id": target})
            message = (
                f"{self.platform_name} API request timed out after "
                f"{self.client.timeout.read}s {operation}."
            )
            return ValueError(f"{message} {hint}".rstrip())

        logger.error(
            f"Failed to connect to {self.platform_name} API for {target}",
            extra={"repo_id": target},
        )
        return ValueError(
            f"Cannot connect to {self.platform_name} API at {self.api_base_url} "
            f"for {target}. Check your internet connection, firewall, "
            f"or {self.platform_name} API status."
        )

    @abstractmethod
    def git_clone_url(self, owner: str, repo: str) -> str:
        """Return the HTTPS git URL used to clone a repository.

        Each platform derives this from the base URL it was already configured
        with, so the workflow layer does not need per-platform hostname rules.

        Args:
            owner: Repository owner / namespace
            repo: Repository name

        Returns:
            HTTPS clone URL, e.g. "https://github.com/owner/repo.git"
        """

    async def get_review_anchor(self, owner: str, repo: str, pr_number: int) -> ReviewAnchor:
        """Fetch the immutable review anchor for a PR/MR (single network call).

        One-shot convenience for callers that do not already hold the PR data.
        A caller that has it (the review workflow, which needs the same payload
        for the LLM prompt) should call :meth:`anchor_from_pr` directly rather
        than paying for a second identical fetch.

        Args:
            owner: Repository owner
            repo: Repository name
            pr_number: Pull request / merge request number

        Returns:
            ReviewAnchor bound to this PR/MR's current head
        """
        pr_data = await self.get_pr(owner, repo, pr_number)
        return self.anchor_from_pr(pr_data, owner, repo, pr_number)

    def anchor_from_pr(self, pr_data: dict, owner: str, repo: str, pr_number: int) -> ReviewAnchor:
        """Derive the review anchor from an already-fetched PR payload.

        Pure derivation, no I/O. Default implementation resolves the head commit
        SHA. Platforms needing richer positioning data (e.g., GitLab diff_refs)
        override this to return their anchor subclass.

        Args:
            pr_data: PR/MR payload as returned by ``get_pr``
            owner: Repository owner
            repo: Repository name
            pr_number: Pull request / merge request number

        Returns:
            ReviewAnchor bound to this PR/MR's current head

        Raises:
            ValueError: If the payload lacks the required positioning data
        """
        head_sha = (pr_data.get("head") or {}).get("sha")
        if not head_sha:
            raise ValueError(
                f"API response for PR #{pr_number} in {owner}/{repo} "
                "missing required 'head.sha' field"
            )
        return ReviewAnchor(
            owner=owner,
            repo=repo,
            pr_number=pr_number,
            commit_sha=head_sha,
        )

    @abstractmethod
    async def create_issue(
        self,
        owner: str,
        repo: str,
        title: str,
        body: str,
        labels: list[str] | None = None,
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

    @abstractmethod
    async def get_pr(self, owner: str, repo: str, pr_number: int) -> dict:
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

    async def post_review_comment(
        self,
        owner: str,
        repo: str,
        pr_number: int,
        file_path: str,
        line: int,
        body: str,
    ) -> None:
        """Post a line-specific review comment on a PR (one-shot convenience).

        Resolves the review anchor and posts a single inline comment through it.
        Concrete here because the composition never varies by platform — the two
        primitives it builds on (``get_review_anchor`` and
        ``create_pr_review_comment``) are where platform differences live.

        Prefer ``get_review_anchor`` + ``create_pr_review_comment`` when posting
        a batch of comments, so the whole review shares one anchor.

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
        anchor = await self.get_review_anchor(owner, repo, pr_number)
        await self.create_pr_review_comment(anchor, file_path, line, body)

    @abstractmethod
    async def create_pr_review_comment(
        self,
        anchor: ReviewAnchor,
        file_path: str,
        line: int,
        body: str,
    ) -> None:
        """Post an inline review comment using a pre-fetched review anchor.

        This is the canonical primitive used by the PR review workflow: the
        anchor (owner/repo/pr_number plus platform positioning data) is
        obtained once per review via ``get_review_anchor`` and reused for
        every inline comment. ``post_review_comment`` is the one-shot variant
        that resolves the anchor first.

        Args:
            anchor: Immutable review anchor (from get_review_anchor)
            file_path: File path relative to repo root
            line: Line number in the new version (after changes)
            body: Comment body (markdown supported)

        Raises:
            ValueError: If review comment creation fails
        """

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

    async def close(self) -> None:
        """Close the adapter and release resources (HTTP connections, etc.).

        Should be called when the adapter is no longer needed to properly
        clean up HTTP clients and other resources.

        Concrete here because every adapter closes the same single
        ``httpx.AsyncClient``. Non-critical errors are logged rather than
        re-raised, so a failure here cannot mask the original exception in a
        ``finally`` block; interrupts and cancellation always propagate.

        Example:
            adapter = GiteaAdapter(url="...", token="...")
            try:
                # Use adapter
                await adapter.create_issue(...)
            finally:
                await adapter.close()
        """
        try:
            await self.client.aclose()
            logger.debug(f"Closed {self.platform_name} adapter HTTP client")
        except (KeyboardInterrupt, SystemExit, asyncio.CancelledError):
            # Always propagate user interrupts, system exit, and async cancellation
            logger.info("Close interrupted by user or system")
            raise
        except (httpx.CloseError, RuntimeError) as e:
            # Expected during close - suppress to avoid masking original errors
            logger.warning(
                f"Non-critical error closing {self.platform_name} client: {e}",
                extra={"error_type": type(e).__name__},
            )
        except Exception as e:
            # Unexpected - log with traceback, but still do not mask the original error
            logger.error(
                f"Unexpected error closing {self.platform_name} adapter: {e}",
                extra={"error_type": type(e).__name__},
                exc_info=True,
            )
