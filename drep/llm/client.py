"""LLM client with advanced rate limiting, caching, and robust JSON parsing.

This module provides a production-ready LLM client that interfaces with OpenAI-compatible
APIs (including local models via LM Studio, Ollama, etc.). It implements sophisticated
features to ensure reliable, efficient, and cost-effective LLM operations:

Key Features:
-------------
1. **Multi-level Rate Limiting**:
   - Global concurrent request limits (prevents overwhelming the LLM server)
   - Per-repository concurrent limits (prevents one repo from monopolizing resources)
   - Requests-per-minute throttling (respects API rate limits)
   - Tokens-per-minute throttling (prevents cost overruns)

2. **Intelligent Caching**:
   - Content-based cache keys with commit SHA invalidation
   - Dramatically reduces costs and latency for repeated analyses
   - Optional - can be disabled if not needed

3. **Robust JSON Parsing**:
   - 5-level fallback strategy for handling malformed LLM responses
   - Markdown fence extraction, common error fixes, truncation recovery
   - Fuzzy inference as last resort

4. **Reliability Features**:
   - Exponential backoff retries with configurable attempts
   - Circuit breaker pattern to prevent cascade failures
   - Comprehensive metrics tracking for observability

5. **Backend Flexibility**:
   - Prefers open-agent-sdk when available (better performance)
   - Falls back to raw HTTP client (universal compatibility)
   - Both implement OpenAI-compatible chat completions API

Architecture:
-------------
The client uses an async context manager pattern for rate limiting, ensuring proper
resource cleanup even when requests fail. Rate limits are enforced using:
- asyncio.Semaphore for concurrency control (global + per-repo)
- Sliding window for requests-per-minute tracking
- Token bucket algorithm for tokens-per-minute enforcement

Usage Example:
--------------

::

    client = LLMClient(
        endpoint="http://localhost:1234/v1",
        model="local-model",
        max_concurrent_global=5,
        max_concurrent_per_repo=3,
        requests_per_minute=60,
        max_tokens_per_minute=MAX_TOKENS_PER_MINUTE,
    )

    # Simple text analysis
    response = await client.analyze_code(
        system_prompt="Review this code for bugs",
        code="def foo(): pass",
        repo_id="my-repo",
    )

    # JSON analysis with schema validation
    result = await client.analyze_code_json(
        system_prompt="Return JSON with findings",
        code="def foo(): pass",
        schema=MyPydanticModel,
    )
"""

import asyncio  # For async/await and concurrency primitives (Semaphore, Lock)
import json  # For parsing LLM JSON responses
import logging  # For structured logging throughout the module
import re  # For regex-based JSON extraction and cleaning
import subprocess  # For executing git commands to get commit SHA
import time  # For rate limiting time calculations
from dataclasses import dataclass  # For simple data classes (LLMResponse)
from pathlib import Path  # For cross-platform file path handling
from typing import Any, Dict, Optional, Type  # Type hints for better IDE support and clarity

import httpx  # Modern async HTTP client (fallback when open-agent-sdk unavailable)
from pydantic import BaseModel  # For JSON schema validation and type safety

from drep.constants import (
    DEFAULT_MAX_TOKENS_PER_REQUEST,
    MAX_ESTIMATED_TOKENS,
    MAX_TOKENS_PER_MINUTE,
    REPO_SEMAPHORE_TTL_SECONDS,
)
from drep.llm.circuit_breaker import CircuitBreaker  # Prevents cascade failures
from drep.llm.metrics import LLMMetrics  # Tracks usage statistics for cost monitoring

logger = logging.getLogger(__name__)

# Sentinel value for distinguishing "not provided" from "explicitly None"
_UNSET = object()


def get_current_commit_sha(repo_path: Optional[Path] = None) -> str:
    """Get current git commit SHA for cache invalidation.

    This function is used to tie cached LLM responses to specific commits. When code
    changes (new commit), the cache is automatically invalidated, ensuring stale
    analysis results aren't reused.

    The function is designed to never fail - it returns "unknown" rather than raising
    exceptions, since cache invalidation is an optimization not a critical feature.

    Args:
        repo_path: Path to git repository (defaults to current directory).
                   Used when analyzing repos other than the one drep is running in.

    Returns:
        Commit SHA string (40-char hex), or "unknown" if:
        - Not in a git repository
        - Git command not available
        - Git command times out (shouldn't happen, but defensive)
        - Any other error occurs

    Note:
        Returns "unknown" for all errors rather than raising exceptions to ensure
        cache operations remain best-effort and don't break the main analysis flow.
    """
    try:
        # Determine which directory to run git command in
        cwd = repo_path if repo_path else Path.cwd()

        # Execute git rev-parse HEAD to get current commit SHA
        # - capture_output=True: Capture stdout/stderr for logging
        # - text=True: Return strings instead of bytes
        # - timeout=5: Prevent hanging on network-mounted repos or weird git configs
        result = subprocess.run(
            ["git", "rev-parse", "HEAD"],
            cwd=cwd,
            capture_output=True,
            text=True,
            timeout=5,
        )

        if result.returncode == 0:
            # Success - return the SHA (strip any trailing newline)
            return result.stdout.strip()
        else:
            # Not a git repository or git not available
            # This is expected when running outside a repo, so only warn-level logging
            logger.warning(f"Could not get commit SHA: {result.stderr}")
            return "unknown"

    except subprocess.TimeoutExpired:
        # Git command took >5 seconds (very rare, possibly network-mounted repo)
        logger.warning("Git command timed out")
        return "unknown"
    except FileNotFoundError:
        # git executable not in PATH (e.g., Windows without Git for Windows)
        logger.warning("Git not found in PATH")
        return "unknown"
    except Exception as e:
        # Catch-all for any other errors (permissions, etc.)
        logger.warning(f"Error getting commit SHA: {e}")
        return "unknown"


@dataclass
class LLMResponse:
    """Structured response from an LLM request with metadata.

    This dataclass wraps the LLM's response content along with usage metrics
    that are useful for:
    - Cost tracking (tokens_used)
    - Performance monitoring (latency_ms)
    - Model version tracking (model)
    - Caching (all fields stored in cache)

    Attributes:
        content: The actual text response from the LLM (e.g., analysis results,
                 generated docstrings, JSON findings). This is the primary output.
        tokens_used: Total tokens consumed by this request (prompt + completion).
                     Used for cost calculation and rate limiting.
        latency_ms: Request latency in milliseconds from request start to response.
                    Used for performance monitoring and SLA tracking.
        model: The actual model name that served the request. May differ from
               requested model if the LLM server does model aliasing.
    """

    content: str  # The LLM's response text
    tokens_used: int  # Total tokens (prompt + completion)
    latency_ms: float  # Request duration in milliseconds
    model: str  # Actual model name used


class RateLimitContext:
    """Async context manager that enforces rate limits for LLM requests.

    This class implements the async context manager protocol (__aenter__/__aexit__)
    to ensure rate limits are properly enforced and resources are cleaned up even
    when requests fail.

    Key Design Decisions:
    ---------------------
    1. **Holds semaphore for entire request duration**: Unlike some rate limiters that
       release the semaphore immediately after acquiring it, this holds it until the
       request completes. This prevents "thundering herd" problems where many requests
       queue up and then all fire simultaneously.

    2. **Two-phase token accounting**: Reserves estimated tokens on entry, then adjusts
       to actual tokens on exit. This prevents token limit bypass when requests are
       queued.

    3. **Graceful failure handling**: If a request fails before completion, the context
       rolls back the estimated token reservation to avoid "leaking" reserved tokens.

    Usage Pattern:
    --------------

    ::

        async with rate_limiter.request(estimated_tokens=1000, repo_id="my-repo") as ctx:
            response = await make_llm_request()
            ctx.set_actual_tokens(response.tokens_used)
            return response

    The context manager ensures:
    - Global concurrency limit is enforced
    - Per-repo concurrency limit is enforced (if repo_id provided)
    - Request rate limit is checked
    - Token rate limit is checked with estimated tokens
    - Actual tokens are reconciled on exit
    - All semaphores are released even if request fails
    """

    def __init__(self, rate_limiter: "RateLimiter", estimated_tokens: int, repo_id: Optional[str]):
        """Initialize rate limit context for a single request.

        Args:
            rate_limiter: Parent RateLimiter instance that manages global state.

            estimated_tokens: Estimated token usage for this request (prompt + max_tokens).
                Used to reserve capacity in the token bucket. Will be adjusted
                to actual usage on exit.

            repo_id: Optional repository identifier for per-repo concurrency limits.
                Multiple requests for the same repo_id will be limited to
                max_concurrent_per_repo, preventing one repo from monopolizing resources.
        """
        self.rate_limiter = rate_limiter
        self.estimated_tokens = estimated_tokens
        self.repo_id = repo_id
        self.actual_tokens: Optional[int] = None  # Set by caller after request completes
        self.repo_semaphore: Optional[asyncio.Semaphore] = None  # Set in __aenter__

    async def __aenter__(self):
        """Acquire semaphores and enforce rate limits before allowing request to proceed.

        This method is called when entering the 'async with' block. It enforces all
        rate limits in sequence:

        1. Global concurrency (wait if max_concurrent_global requests already running)
        2. Per-repo concurrency (wait if max_concurrent_per_repo for this repo already running)
        3. Request rate limit (wait if requests_per_minute exceeded)
        4. Token rate limit (wait if adding this request would exceed max_tokens_per_minute)

        All semaphores are held until __aexit__, ensuring proper concurrency control.

        Returns:
            self: Allows accessing the context object in the 'as' clause

        Note:
            This method may sleep (via await) if rate limits are currently exceeded.
            The sleeps are calculated to wait until limits reset (e.g., wait until
            oldest request in the 1-minute window expires).
        """
        # STEP 1: Acquire global semaphore (limits total concurrent requests)
        # This blocks if max_concurrent_global requests are already in flight
        # Semaphore is held until __aexit__ to ensure proper concurrency control
        await self.rate_limiter.semaphore.acquire()

        # STEP 2: Acquire per-repo semaphore if repo_id specified
        # This prevents one repository from monopolizing all concurrent slots
        # Example: If max_concurrent_global=5 and max_concurrent_per_repo=3,
        # then repo A can use at most 3 slots, leaving 2+ for other repos
        if self.repo_id is not None:
            self.repo_semaphore = await self.rate_limiter._get_repo_semaphore(self.repo_id)
            await self.repo_semaphore.acquire()

        # STEP 3: Check request rate limit (requests per minute)
        # This uses a sliding window algorithm: track timestamps of recent requests
        # and wait if adding this request would exceed the per-minute limit
        await self.rate_limiter._check_request_rate_limit()

        # STEP 4: Check token rate limit (tokens per minute)
        # This uses a token bucket algorithm: track cumulative tokens in current minute
        # and wait if adding estimated tokens would exceed the limit
        # We use estimated tokens here; actual tokens reconciled in __aexit__
        await self.rate_limiter._check_token_rate_limit(self.estimated_tokens)

        return self

    async def __aexit__(self, exc_type, exc_val, exc_tb):
        """Release semaphores and reconcile actual token usage.

        This method is called when exiting the 'async with' block, either normally
        or due to an exception. It performs cleanup and token accounting:

        1. Reconcile token usage: Replace estimated tokens with actual tokens (if known)
        2. Release per-repo semaphore (if acquired)
        3. Release global semaphore

        Token Reconciliation Example:
        -----------------------------
        Entry: estimated_tokens=1000, tokens_used=5000
        Exit:  actual_tokens=800
        Result: tokens_used = 5000 - 1000 + 800 = 4800

        This ensures the token bucket reflects actual usage, not pessimistic estimates.

        Args:
            exc_type: Exception type if an error occurred, None otherwise
            exc_val: Exception value if an error occurred, None otherwise
            exc_tb: Exception traceback if an error occurred, None otherwise

        Note:
            This method does NOT propagate exceptions (returns None implicitly),
            so any exception that occurred in the 'async with' block will continue
            to propagate after cleanup.
        """
        # STEP 1: Reconcile token accounting under lock (to prevent race conditions)
        async with self.rate_limiter.lock:
            if self.actual_tokens is not None:
                # Request completed successfully and actual tokens were set
                # Adjust token count: remove estimate, add actual
                # Formula: tokens_used = tokens_used - estimated + actual
                # Clamp to 0 to handle edge case where bucket was reset during request
                self.rate_limiter.tokens_used = max(
                    0, self.rate_limiter.tokens_used - self.estimated_tokens
                )
                self.rate_limiter.tokens_used += self.actual_tokens
                # Log at debug level for token accounting audit trail
                logger.debug(
                    f"Token reconciliation: estimated={self.estimated_tokens}, "
                    f"actual={self.actual_tokens}, "
                    f"total={self.rate_limiter.tokens_used}"
                )
            else:
                # Request failed before set_actual_tokens() was called
                # Roll back the estimated token reservation to avoid "leaking" capacity
                # This is critical: without this, failed requests would permanently
                # reduce available tokens in the bucket
                self.rate_limiter.tokens_used = max(
                    0, self.rate_limiter.tokens_used - self.estimated_tokens
                )
                logger.debug(
                    f"Rolling back {self.estimated_tokens} token reservation "
                    f"(request failed without completion)"
                )

        # STEP 2: Release per-repo semaphore if it was acquired
        # This allows other requests for the same repo to proceed
        if self.repo_semaphore is not None:
            self.repo_semaphore.release()

        # STEP 3: Release global semaphore
        # This allows other requests (any repo) to proceed
        # Released last to ensure proper cleanup order
        self.rate_limiter.semaphore.release()

    def set_actual_tokens(self, tokens: int):
        """Set actual token usage after request completes.

        This should be called by the request handler after getting the response,
        before exiting the context manager. If not called (e.g., request failed),
        the estimated tokens will be rolled back in __aexit__.

        Args:
            tokens: Actual total tokens used by the request (prompt + completion).
                   Obtained from the LLM API response's usage field.

        Example:
            async with rate_limiter.request(1000) as ctx:
                response = await llm_client.complete(...)
                ctx.set_actual_tokens(response.usage.total_tokens)
        """
        self.actual_tokens = tokens


class RateLimiter:
    """Dual-bucket rate limiter with multi-level concurrency control.

    This class implements a sophisticated rate limiting system that prevents:
    1. Overwhelming LLM servers (global concurrency limit)
    2. Resource monopolization by one repo (per-repo concurrency limits)
    3. API rate limit violations (requests per minute)
    4. Cost overruns (tokens per minute)

    Algorithms Used:
    ----------------
    - **Concurrency Control**: asyncio.Semaphore (counting semaphore pattern)
      Limits number of requests that can run simultaneously

    - **Request Rate Limiting**: Sliding Window Algorithm
      Tracks timestamps of recent requests in a list. Before allowing a new request,
      removes timestamps >60s old. If list length >= limit, waits until oldest expires.

    - **Token Rate Limiting**: Token Bucket Algorithm (fixed window variant)
      Tracks cumulative tokens used in current minute. Resets counter every 60s.
      Before allowing a request, checks if adding estimated tokens would exceed limit.

    Memory Management:
    ------------------
    The per-repo semaphore dictionary could grow unbounded if many repos are scanned.
    To prevent memory leaks, idle semaphores (not used for 10+ minutes) are periodically
    evicted. This only happens when not in use (all permits available).

    Thread Safety:
    --------------
    All rate limit checks and updates are protected by self.lock (asyncio.Lock)
    to prevent race conditions in concurrent environments.

    Example Configuration:
    ----------------------

    ::

        limiter = RateLimiter(
            max_concurrent=5,              # 5 requests in flight max
            requests_per_minute=60,        # 60 reqs/min = 1 req/sec average
            max_tokens_per_minute=MAX_TOKENS_PER_MINUTE,  # 100K tokens/min limit
            max_concurrent_per_repo=3,     # Each repo limited to 3 concurrent
        )
    """

    def __init__(
        self,
        max_concurrent: int,
        requests_per_minute: int,
        max_tokens_per_minute: int,
        max_concurrent_per_repo: Optional[int] = None,
    ):
        """Initialize rate limiter with specified limits.

        Args:
            max_concurrent: Maximum concurrent requests globally. This is the total
                number of LLM requests that can be in-flight simultaneously
                across all repositories. Example: 5 means at most 5 requests
                running at once.
            requests_per_minute: Maximum requests per minute. Uses sliding window
                algorithm to enforce. Example: 60 means 60 requests
                in any 60-second window.
            max_tokens_per_minute: Maximum tokens per minute. Uses token bucket
                algorithm (fixed window). Example: 100000 means
                100K tokens consumed in current minute before
                throttling.
            max_concurrent_per_repo: Maximum concurrent requests per repository.
                If None, defaults to max_concurrent (no per-repo
                limit). Example: 3 means each repo can use at most
                3 of the global concurrent slots.

        Note:
            All limits are soft limits - they're enforced by sleeping/waiting rather
            than rejecting requests. This ensures no request is ever lost, just delayed.
        """
        # Store configuration
        self.max_concurrent = max_concurrent
        self.requests_per_minute = requests_per_minute
        self.max_tokens_per_minute = max_tokens_per_minute
        self.max_concurrent_per_repo = max_concurrent_per_repo

        # Global concurrency control: Semaphore with N permits
        # Each request acquires a permit, blocks if none available, releases on completion
        self.semaphore = asyncio.Semaphore(max_concurrent)

        # Per-repository concurrency control
        # Maps repo_id -> Semaphore to limit concurrent requests per repository
        # This prevents one busy repo from using all global concurrent slots
        self.repo_semaphores: Dict[str, asyncio.Semaphore] = {}

        # Track last access time for each repo's semaphore (for cleanup)
        # Maps repo_id -> timestamp (seconds since epoch)
        self.repo_last_used: Dict[str, float] = {}

        # Time-to-live for idle repo semaphores: 10 minutes
        # After 10 minutes of inactivity, a repo's semaphore is eligible for eviction
        # This prevents memory leaks when scanning many repos over time
        self.repo_semaphore_ttl = REPO_SEMAPHORE_TTL_SECONDS

        # Request rate limiting: Sliding window algorithm
        # Lock protects all shared state from concurrent access
        self.lock = asyncio.Lock()

        # List of request timestamps (seconds since epoch) in the last minute
        # This list is continuously pruned to remove timestamps >60s old
        # Length of list = number of requests in last 60 seconds
        self.request_times: list[float] = []

        # Token rate limiting: Token bucket algorithm (fixed window variant)
        # Tracks cumulative tokens used in current minute
        self.tokens_used = 0

        # Timestamp when token counter resets (60 seconds from now)
        # When time.time() >= token_reset_time, counter resets to 0
        self.token_reset_time = time.time() + 60

    async def _get_repo_semaphore(self, repo_id: str) -> asyncio.Semaphore:
        """Get or create semaphore for a repository with automatic cleanup.

        This method implements lazy initialization of per-repo semaphores and periodic
        cleanup to prevent memory leaks when scanning many repositories.

        Memory Management Strategy:
        ---------------------------
        Problem: If we create a semaphore for every repo ever scanned, memory usage
        grows unbounded. A server scanning 1000s of repos would accumulate 1000s of
        semaphores.

        Solution: Lazy-create semaphores on first use, and evict idle ones after 10
        minutes. Eviction is safe because:
        1. Only evict if semaphore not currently held (all permits available)
        2. If repo is accessed again, semaphore will be recreated

        This achieves O(active_repos) memory instead of O(total_repos_ever_scanned).

        Args:
            repo_id: Repository identifier (e.g., "owner/repo" or URL)

        Returns:
            Semaphore for the repository, newly created or existing

        Note:
            This method is async and acquires self.lock, so it should not be called
            while holding the lock elsewhere (would cause deadlock).
        """
        async with self.lock:  # Ensure thread-safe access to shared dictionaries
            now = time.time()

            # CLEANUP PHASE: Identify and evict idle semaphores
            # Build list of repos that haven't been used in >10 minutes
            idle_repos = [
                rid
                for rid, last_used in self.repo_last_used.items()
                if now - last_used > self.repo_semaphore_ttl
            ]

            # Evict idle semaphores (only if not currently in use)
            for rid in idle_repos:
                sem = self.repo_semaphores.get(rid)
                if sem is not None:
                    # Check if semaphore is idle: _value == initial value means
                    # all permits available (no requests using this repo's semaphore)
                    expected_idle_value = self.max_concurrent_per_repo or self.max_concurrent
                    if sem._value == expected_idle_value:
                        # Safe to evict - no requests in flight for this repo
                        del self.repo_semaphores[rid]
                        del self.repo_last_used[rid]
                        logger.debug(f"Evicted idle semaphore for repo {rid}")

            # INITIALIZATION PHASE: Get or create semaphore for requested repo
            if repo_id not in self.repo_semaphores:
                # Semaphore doesn't exist yet (first request for this repo, or was evicted)
                # Create new semaphore with appropriate limit
                limit = self.max_concurrent_per_repo or self.max_concurrent
                self.repo_semaphores[repo_id] = asyncio.Semaphore(limit)
                logger.debug(f"Created semaphore for repo {repo_id} with limit {limit}")

            # Update last access time to prevent eviction while actively used
            self.repo_last_used[repo_id] = now

            return self.repo_semaphores[repo_id]

    async def _check_request_rate_limit(self):
        """Check and enforce request rate limit using sliding window algorithm.

        Sliding Window Algorithm:
        --------------------------
        Tracks exact timestamps of recent requests in a list. Before allowing a new
        request:
        1. Prune list: Remove timestamps older than 60 seconds
        2. Check count: If len(list) >= limit, window is full
        3. Calculate wait: If full, wait until oldest timestamp expires
        4. Record: Add current timestamp to list

        This gives exact rate limiting over any 60-second window, unlike fixed-window
        algorithms which can allow 2x burst at window boundaries.

        Example:
        --------
        Limit: 60 req/min
        Scenario: 60 requests at t=0s
        - At t=30s: list still has 60 entries (none >60s old), blocks new requests
        - At t=61s: list empty (all entries >60s old), allows 60 more requests

        This prevents burst scenarios where fixed-window would allow 120 req/min
        (60 at end of minute 1, 60 at start of minute 2).

        Note:
            This method acquires self.lock and may sleep (await), so do not call
            while holding the lock elsewhere.
        """
        async with self.lock:  # Protect shared request_times list from concurrent access
            now = time.time()

            # PRUNE PHASE: Remove timestamps older than 60 seconds
            # List comprehension creates new list with only recent timestamps
            # This maintains a sliding 60-second window
            self.request_times = [t for t in self.request_times if now - t < 60]

            # ENFORCEMENT PHASE: Wait if rate limit currently exceeded
            while len(self.request_times) >= self.requests_per_minute:
                # Window is full - must wait for oldest request to expire
                oldest = self.request_times[0]  # Earliest timestamp in window

                # Calculate how long until oldest timestamp is 60s old
                wait_time = 60 - (now - oldest)

                if wait_time > 0:
                    # Still need to wait - sleep until oldest expires
                    logger.debug(f"Request rate limit reached, waiting {wait_time:.1f}s")
                    await asyncio.sleep(wait_time)

                # Re-check after sleep - refresh 'now' and prune again
                # (Other coroutines may have modified request_times while we slept)
                now = time.time()
                self.request_times = [t for t in self.request_times if now - t < 60]

            # RECORD PHASE: Add current request to sliding window
            self.request_times.append(now)

    async def _check_token_rate_limit(self, estimated_tokens: int):
        """Check and enforce token rate limit using token bucket algorithm.

        Token Bucket Algorithm (Fixed Window Variant):
        -----------------------------------------------
        Maintains a counter of tokens used in the current minute. The counter resets
        every 60 seconds. Before allowing a request:
        1. Check if current minute expired: Reset counter if needed
        2. Check capacity: If current_usage + estimated > limit, bucket is full
        3. Calculate wait: If full, wait until next minute (bucket reset)
        4. Reserve: Add estimated tokens to current usage

        Trade-offs:
        -----------
        - Pro: Simple, predictable, easy to reason about costs
        - Con: Allows bursts at minute boundaries (all tokens in first second)
        - Alternative: Sliding window token algorithm (more complex, smoother)

        We use fixed window because:
        1. LLM providers typically use fixed-window rate limits (easier to match)
        2. Burst behavior is acceptable for our use case (scanning is bursty)
        3. Simpler implementation with less memory overhead

        Example:
        --------
        Limit: 100K tokens/min
        - t=0s: Use 80K tokens (tokens_used=80K)
        - t=30s: Try to use 30K tokens (80K+30K > 100K, so wait)
        - t=60s: Counter resets (tokens_used=0), can use 30K tokens

        Args:
            estimated_tokens: Estimated tokens for the upcoming request. This is
                            the sum of prompt tokens and max_tokens setting.

        Note:
            Actual tokens are reconciled after request completes (may be less than
            estimated). See RateLimitContext.__aexit__ for reconciliation logic.
        """
        async with self.lock:  # Protect shared token counters from concurrent access
            now = time.time()

            # RESET PHASE: Check if current minute has expired
            if now >= self.token_reset_time:
                # Minute boundary crossed - reset counter and update reset time
                self.tokens_used = 0
                self.token_reset_time = now + 60
                logger.debug(f"Token bucket reset at {now}")

            # ENFORCEMENT PHASE: Wait if adding this request would exceed limit
            while self.tokens_used + estimated_tokens > self.max_tokens_per_minute:
                # Bucket is full - must wait for next minute
                wait_time = self.token_reset_time - now

                if wait_time > 0:
                    # Still in current minute - sleep until reset
                    logger.debug(
                        f"Token rate limit reached "
                        f"({self.tokens_used}/{self.max_tokens_per_minute}), "
                        f"waiting {wait_time:.1f}s"
                    )
                    await asyncio.sleep(wait_time)

                # Re-check after sleep - reset counter and update reset time
                # (We've now entered a new minute)
                now = time.time()
                self.tokens_used = 0
                self.token_reset_time = now + 60

            # RESERVATION PHASE: Reserve estimated tokens for this request
            # This prevents token limit bypass when multiple requests are queued
            # Actual tokens will be reconciled in RateLimitContext.__aexit__
            self.tokens_used += estimated_tokens
            logger.debug(
                f"Reserved {estimated_tokens} tokens "
                f"(total: {self.tokens_used}/{self.max_tokens_per_minute})"
            )

    def request(self, estimated_tokens: int, repo_id: Optional[str] = None):
        """Create a rate-limited context manager for an LLM request.

        This is the main entry point for rate limiting. Call this method to get
        a context manager that enforces all rate limits.

        Args:
            estimated_tokens: Estimated total tokens for this request (prompt + max_tokens).
                            Used for token rate limiting. Will be reconciled with actual
                            tokens after request completes.
            repo_id: Optional repository identifier for per-repo concurrency limiting.
                    If provided, enforces max_concurrent_per_repo in addition to global
                    limits. If None, only global limits apply.

        Returns:
            RateLimitContext: An async context manager. Use with 'async with' statement.

        Example:
            async with rate_limiter.request(estimated_tokens=1500, repo_id="my/repo") as ctx:
                response = await make_llm_request()
                ctx.set_actual_tokens(response.usage.total_tokens)
                return response
        """
        return RateLimitContext(self, estimated_tokens, repo_id)


class LLMClient:
    r"""Production-ready LLM client for OpenAI-compatible APIs with advanced features.

    This is the main entry point for all LLM operations in drep. It provides a robust,
    production-ready interface that handles the complexities of LLM integration:

    Core Features:
    --------------
    1. **Dual Backend Support**:
       - Prefers open-agent-sdk when available (better performance, native OpenAI SDK)
       - Falls back to raw HTTP via httpx (universal compatibility)
       - Transparent switching - same interface for both

    2. **Multi-level Rate Limiting** (see RateLimiter class):
       - Global concurrency limits (don't overwhelm LLM server)
       - Per-repo concurrency limits (fair resource sharing)
       - Requests-per-minute throttling (respect API limits)
       - Tokens-per-minute throttling (cost control)

    3. **Intelligent Caching**:
       - Content-based keys: (prompt + code + model + temperature + commit_sha)
       - Automatic invalidation on code changes (new commit)
       - Typical cache hit rate: 80%+ on incremental scans
       - Dramatic cost and latency reduction

    4. **Robust JSON Parsing** (5-level fallback strategy):
       - Level 1: Extract from markdown code fences (\`\`\`json)
       - Level 2: Direct JSON parse
       - Level 3: Fix common errors (trailing commas, single quotes)
       - Level 4: Recover truncated JSON (add missing brackets)
       - Level 5: Fuzzy inference from schema (last resort)

    5. **Reliability Features**:
       - Exponential backoff retries (configurable attempts and delays)
       - Circuit breaker pattern (optional, prevents cascade failures)
       - Comprehensive metrics tracking (cost, latency, success rates)
       - Graceful degradation

    Architecture:
    -------------
    The client uses dependency injection for caching and follows the async/await
    pattern throughout. Rate limiting is enforced via async context managers
    that hold semaphores for the entire request duration.

    Typical Request Flow:
    ---------------------
    1. Check cache (if enabled) → return immediately if hit
    2. Acquire rate limit context (may sleep if limits exceeded)
    3. Make LLM API request (with retries on failure)
    4. Update metrics (tokens, latency, success/failure)
    5. Store in cache (if enabled)
    6. Release rate limit context (reconcile actual tokens)
    7. Return response

    Usage Examples:
    ---------------

    ::

        # Initialize with local LLM (LM Studio, Ollama, etc.)
        client = LLMClient(
            endpoint="http://localhost:1234/v1",
            model="local-model",
            api_key="not-needed",  # Many local LLMs don't need keys
            max_concurrent_global=5,
            requests_per_minute=60,
            max_tokens_per_minute=MAX_TOKENS_PER_MINUTE,
        )

        # Simple text analysis
        response = await client.analyze_code(
            system_prompt="Review this Python code for bugs",
            code="def divide(a, b): return a / b",
            repo_id="my-org/my-repo",
        )
        print(f"Analysis: {response.content}")
        print(f"Tokens used: {response.tokens_used}")

        # JSON analysis with schema validation
        from pydantic import BaseModel
        class BugReport(BaseModel):
            bugs: list[str]
            severity: str

        result = await client.analyze_code_json(
            system_prompt="Return JSON: {bugs: [...], severity: 'high'|'medium'|'low'}",
            code="def divide(a, b): return a / b",
            schema=BugReport,  # Validates and provides fallback parsing
        )
        print(f"Found {len(result['bugs'])} bugs")

        # Don't forget to close when done
        await client.close()
    """

    def __init__(
        self,
        endpoint: str,
        model: str,
        api_key: Optional[str] = None,
        temperature: float = 0.2,
        max_tokens: int = DEFAULT_MAX_TOKENS_PER_REQUEST,
        timeout: int = 60,
        max_retries: int = 3,
        retry_delay: int = 2,
        exponential_backoff: bool = True,
        max_concurrent_global: int = 5,
        max_concurrent_per_repo: Optional[int] = 3,
        requests_per_minute: int = 60,
        max_tokens_per_minute: int = MAX_TOKENS_PER_MINUTE,
        cache: Optional["IntelligentCache"] = None,  # noqa: F821
        repo_path: Optional[Path] = None,
        rate_limiter: Optional[RateLimiter] = None,
        enable_circuit_breaker: bool = True,
        circuit_breaker: Optional[CircuitBreaker] = _UNSET,  # type: ignore
        circuit_breaker_threshold: int = 5,
        circuit_breaker_timeout: int = 60,
        provider: str = "openai-compatible",
        bedrock_region: Optional[str] = None,
        bedrock_model: Optional[str] = None,
    ):
        """Initialize LLM client.

        Args:
            endpoint: OpenAI-compatible API endpoint
            model: Model name to use
            api_key: Optional API key
            temperature: Sampling temperature (0.0-2.0)
            max_tokens: Maximum tokens per request
            timeout: Request timeout in seconds
            max_retries: Maximum retry attempts
            retry_delay: Initial retry delay in seconds
            exponential_backoff: Use exponential backoff for retries

            max_concurrent_global: Maximum concurrent requests globally
                (ignored if rate_limiter provided)

            max_concurrent_per_repo: Maximum concurrent requests per repository
                (ignored if rate_limiter provided)

            requests_per_minute: Rate limit for requests
                (ignored if rate_limiter provided)

            max_tokens_per_minute: Rate limit for tokens
                (ignored if rate_limiter provided)

            cache: Optional cache instance for response caching
            repo_path: Optional repository path for commit SHA retrieval
            rate_limiter: Optional RateLimiter instance (creates default if None)

            enable_circuit_breaker: Enable circuit breaker pattern
                (ignored if circuit_breaker provided)

            circuit_breaker: Optional CircuitBreaker instance
                (None to disable, creates default if not provided)

            circuit_breaker_threshold: Failures before opening circuit
                (ignored if circuit_breaker provided)

            circuit_breaker_timeout: Recovery timeout in seconds
                (ignored if circuit_breaker provided)
        """
        # Store configuration parameters
        # Bedrock doesn't need endpoint, so handle None gracefully
        self.endpoint = endpoint.rstrip("/") if endpoint else None
        self.model = model  # Model name (e.g., "gpt-4", "llama-2-70b", etc.)
        self.temperature = temperature  # Sampling temperature: lower = more deterministic
        self.max_tokens = max_tokens  # Maximum completion tokens per request
        self.timeout = timeout  # HTTP timeout in seconds
        self.max_retries = max_retries  # Number of retry attempts on failure
        self.retry_delay = retry_delay  # Initial delay between retries (seconds)
        self.exponential_backoff = exponential_backoff  # Whether to use exponential backoff
        self.cache = cache  # Optional IntelligentCache instance for response caching
        self.repo_path = repo_path  # Optional repo path for commit SHA retrieval
        self._provider = provider  # LLM provider: openai-compatible, bedrock, anthropic

        # === PROVIDER SELECTION: Bedrock, open-agent-sdk, or HTTP ===
        # Check if Bedrock provider is requested
        if provider == "bedrock":
            from drep.llm.providers.bedrock_client import BedrockClient

            if not bedrock_region or not bedrock_model:
                raise ValueError(
                    "Bedrock provider requires bedrock_region and bedrock_model parameters"
                )

            self.bedrock_client = BedrockClient(
                region=bedrock_region,
                model=bedrock_model,
            )
            self._using_bedrock = True

            # CRITICAL: Preserve Bedrock model in self.model for cache keys and metadata
            # Without this, cache lookups use model=None and different Bedrock models
            # can serve stale cached results from each other
            self.model = bedrock_model

            logger.info(
                f"LLM backend: AWS Bedrock (region={bedrock_region}, model={bedrock_model})"
            )

            # Initialize rate limiter (use injected or create default)
            if rate_limiter is not None:
                self.rate_limiter = rate_limiter
            else:
                self.rate_limiter = RateLimiter(
                    max_concurrent=max_concurrent_global,
                    requests_per_minute=requests_per_minute,
                    max_tokens_per_minute=max_tokens_per_minute,
                    max_concurrent_per_repo=max_concurrent_per_repo,
                )

            # Metrics tracking
            self.metrics = LLMMetrics()

            # Circuit breaker (optional, use injected or create default)
            if circuit_breaker is not _UNSET:
                self.circuit_breaker = circuit_breaker  # type: ignore
            elif enable_circuit_breaker:
                self.circuit_breaker = CircuitBreaker(
                    failure_threshold=circuit_breaker_threshold,
                    recovery_timeout=circuit_breaker_timeout,
                )
            else:
                self.circuit_breaker = None

            return  # Skip open-agent-sdk/HTTP initialization

        # === BACKEND SELECTION: open-agent-sdk vs HTTP ===
        # We support two backends with identical interfaces:
        # 1. open-agent-sdk: Preferred, better performance, more features
        # 2. HTTP (httpx): Fallback, universal compatibility

        self._using_open_agent = False  # Flag to track which backend is active
        self.client = None  # Will be AsyncOpenAI instance or compat shim

        # Try to initialize open-agent-sdk (preferred backend)
        try:
            from open_agent.types import AgentOptions  # type: ignore
            from open_agent.utils import create_client  # type: ignore

            # Configure open-agent-sdk with our settings
            options = AgentOptions(
                system_prompt="",  # System prompt is provided per-request, not here
                model=self.model,
                base_url=self.endpoint,
                timeout=self.timeout,
                api_key=api_key or "not-needed",  # Local LLMs often don't need keys
            )
            self.client = create_client(options)  # Returns AsyncOpenAI-compatible instance
            self._using_open_agent = True
            logger.info("LLM backend: open-agent-sdk (OpenAI-compatible)")

        except ImportError:
            # open-agent-sdk not installed - this is fine, we'll use HTTP fallback
            logger.info("LLM backend: HTTP (OpenAI-compatible), open-agent-sdk not installed")
        except Exception as e:
            # open-agent-sdk is installed but failed to initialize (config error, etc.)
            # Fall back to HTTP to ensure we can still operate
            logger.warning(f"open-agent-sdk initialization failed, falling back to HTTP: {e}")

        # Initialize HTTP client (used when open-agent-sdk unavailable)
        self.http = None
        if not self._using_open_agent:
            # Build HTTP headers for OpenAI-compatible API
            headers = {}
            if api_key:
                # Most LLM APIs use Bearer token authentication
                headers["Authorization"] = f"Bearer {api_key}"
            headers["Content-Type"] = "application/json"

            # Create async HTTP client with base URL and headers
            # This will be used to make POST requests to /chat/completions
            self.http = httpx.AsyncClient(base_url=self.endpoint, headers=headers, timeout=timeout)

        # === COMPATIBILITY SHIM FOR HTTP BACKEND ===
        # The following classes create an OpenAI SDK-like interface for our HTTP client.
        # This allows us to use the same code path regardless of backend:
        #     response = await self.client.chat.completions.create(...)
        #
        # Works for both:
        # - open-agent-sdk: Already provides this interface (AsyncOpenAI)
        # - HTTP backend: We create this interface via nested compat classes
        #
        # This also makes testing easier - tests can mock client.chat.completions.create
        # uniformly without caring which backend is active.
        #
        # The shim wraps raw HTTP responses in OpenAI-like objects:
        #     HTTP response → _CompatResponse → response.choices[0].message.content
        client_self = self

        class _CompatMessage:
            def __init__(self, content: str):
                self.content = content

        class _CompatChoice:
            def __init__(self, content: str):
                self.message = _CompatMessage(content)

        class _CompatUsage:
            def __init__(self, usage: Dict[str, Any]):
                prompt = usage.get("prompt_tokens") or usage.get("input_tokens") or 0
                completion = usage.get("completion_tokens") or usage.get("output_tokens") or 0
                self.prompt_tokens = prompt
                self.completion_tokens = completion
                self.total_tokens = usage.get("total_tokens") or (prompt + completion)

        class _CompatResponse:
            def __init__(self, data: Dict[str, Any]):
                self.model = data.get("model", client_self.model)
                content = (
                    ((data.get("choices") or [{}])[0].get("message") or {}).get("content")
                    or data.get("content")
                    or ""
                )
                self.choices = [_CompatChoice(content)]
                self.usage = _CompatUsage(data.get("usage", {}))

        class _CompatCompletions:
            def __init__(self, parent: "LLMClient"):
                self._parent = parent

            async def create(self, model: str, messages: list, temperature: float, max_tokens: int):
                if not self._parent.http:
                    raise RuntimeError("HTTP client not initialized")
                url = f"{self._parent.endpoint}/chat/completions"
                payload = {
                    "model": model,
                    "messages": messages,
                    "temperature": temperature,
                    "max_tokens": max_tokens,
                }
                resp = await self._parent.http.post(url, json=payload)
                resp.raise_for_status()
                return _CompatResponse(resp.json())

        class _CompatChat:
            def __init__(self, parent: "LLMClient"):
                self.completions = _CompatCompletions(parent)

        class _CompatClient:
            def __init__(self, parent: "LLMClient"):
                self.chat = _CompatChat(parent)

            async def close(self):
                if parent.http:
                    await parent.http.aclose()

        parent = self
        if not self._using_open_agent:
            self.client = _CompatClient(self)

        # Initialize rate limiter (use injected or create default)
        if rate_limiter is not None:
            # Use injected RateLimiter (dependency injection)
            self.rate_limiter = rate_limiter
        else:
            # Create default RateLimiter (backward compatibility)
            self.rate_limiter = RateLimiter(
                max_concurrent=max_concurrent_global,
                requests_per_minute=requests_per_minute,
                max_tokens_per_minute=max_tokens_per_minute,
                max_concurrent_per_repo=max_concurrent_per_repo,
            )

        # Metrics tracking
        self.metrics = LLMMetrics()

        # Circuit breaker (optional, use injected or create default)
        if circuit_breaker is not _UNSET:
            # Explicitly provided (can be an instance or None to disable)
            # This takes precedence over enable_circuit_breaker flag
            self.circuit_breaker = circuit_breaker  # type: ignore
        elif enable_circuit_breaker:
            # Not provided, use enable_circuit_breaker flag (backward compatibility)
            self.circuit_breaker = CircuitBreaker(
                failure_threshold=circuit_breaker_threshold,
                recovery_timeout=circuit_breaker_timeout,
            )
        else:
            # Circuit breaker disabled via flag
            self.circuit_breaker = None

    async def analyze_code(
        self,
        system_prompt: str,
        code: str,
        repo_id: Optional[str] = None,
        commit_sha: Optional[str] = None,
        analyzer: str = "unknown",
    ) -> LLMResponse:
        """Analyze code with LLM.

        Args:
            system_prompt: System prompt describing the task
            code: Code to analyze
            repo_id: Optional repository identifier
            commit_sha: Optional commit SHA (auto-detected if not provided)
            analyzer: Name of the analyzer making the request

        Returns:
            LLMResponse with content and metadata

        Raises:
            Exception: If all retries fail
        """
        # Get commit SHA if not provided
        if commit_sha is None:
            commit_sha = get_current_commit_sha(self.repo_path)

        # Check cache if available
        if self.cache:
            cached = self.cache.get(
                prompt=system_prompt,
                code=code,
                model=self.model,
                temperature=self.temperature,
                commit_sha=commit_sha,
            )
            if cached:
                logger.debug("Cache hit for analyze_code")
                # Record cached request
                self.metrics.record_request(
                    analyzer=analyzer,
                    success=True,
                    cached=True,
                    tokens_prompt=0,
                    tokens_completion=cached["tokens_used"],
                    latency_ms=0,
                )
                return LLMResponse(
                    content=cached["content"],
                    tokens_used=cached["tokens_used"],
                    latency_ms=cached["latency_ms"],
                    model=cached["model"],
                )

        # Estimate tokens (rough: 4 chars per token), clamp to avoid over-reservation
        estimated_tokens = (len(system_prompt) + len(code) + self.max_tokens) // 4
        estimated_tokens = max(1, min(estimated_tokens, MAX_ESTIMATED_TOKENS))

        # Retry logic
        last_exception = None
        for attempt in range(self.max_retries):
            try:
                async with self.rate_limiter.request(estimated_tokens, repo_id) as ctx:
                    # Make request
                    start_time = time.time()

                    # Use Bedrock provider if configured
                    if self._provider == "bedrock":
                        response = await self.bedrock_client.chat_completion(
                            messages=[
                                {"role": "system", "content": system_prompt},
                                {"role": "user", "content": code},
                            ],
                            temperature=self.temperature,
                            max_tokens=self.max_tokens,
                        )
                    else:
                        # Use OpenAI-compatible provider (open-agent-sdk or HTTP)
                        response = await self.client.chat.completions.create(
                            model=self.model,
                            messages=[
                                {"role": "system", "content": system_prompt},
                                {"role": "user", "content": code},
                            ],
                            temperature=self.temperature,
                            max_tokens=self.max_tokens,
                        )

                    latency_ms = (time.time() - start_time) * 1000

                    # Extract response (handle both dict and object formats)
                    if self._provider == "bedrock":
                        # Bedrock returns dict
                        content = response["choices"][0]["message"]["content"]
                        tokens_used = response["usage"]["total_tokens"]
                        prompt_tokens = response["usage"]["prompt_tokens"]
                        completion_tokens = response["usage"]["completion_tokens"]
                    else:
                        # OpenAI-compatible returns object
                        content = response.choices[0].message.content
                        tokens_used = response.usage.total_tokens
                        prompt_tokens = response.usage.prompt_tokens
                        completion_tokens = response.usage.completion_tokens

                    # Update actual tokens
                    ctx.set_actual_tokens(tokens_used)

                    # Record metrics
                    self.metrics.record_request(
                        analyzer=analyzer,
                        success=True,
                        cached=False,
                        tokens_prompt=prompt_tokens,
                        tokens_completion=completion_tokens,
                        latency_ms=latency_ms,
                    )

                    # Create response object
                    llm_response = LLMResponse(
                        content=content,
                        tokens_used=tokens_used,
                        latency_ms=latency_ms,
                        model=self.model if self._provider == "bedrock" else response.model,
                    )

                    # Cache response if available
                    if self.cache:
                        self.cache.set(
                            prompt=system_prompt,
                            code=code,
                            model=self.model,
                            temperature=self.temperature,
                            commit_sha=commit_sha,
                            response={
                                "content": content,
                                "tokens_used": tokens_used,
                                "latency_ms": latency_ms,
                                "model": (
                                    self.model if self._provider == "bedrock" else response.model
                                ),
                            },
                            tokens_used=tokens_used,
                            latency_ms=latency_ms,
                        )

                    return llm_response

            except Exception as e:
                last_exception = e

                # Record failed request
                self.metrics.record_request(
                    analyzer=analyzer,
                    success=False,
                    cached=False,
                    tokens_prompt=0,
                    tokens_completion=0,
                    latency_ms=0,
                )

                if attempt < self.max_retries - 1:
                    # Calculate backoff delay
                    if self.exponential_backoff:
                        delay = self.retry_delay * (2**attempt)
                    else:
                        delay = self.retry_delay

                    # Sanitize error message to avoid logging tokens in URLs
                    error_msg = str(e)
                    # Basic sanitization: remove common token patterns from error messages
                    import re

                    error_msg = re.sub(
                        r"(token|api_?key|password|secret)=[^&\s]+",
                        r"\1=***",
                        error_msg,
                        flags=re.IGNORECASE,
                    )
                    error_msg = re.sub(r"://[^:]+:[^@]+@", r"://***:***@", error_msg)

                    logger.warning(
                        f"LLM request failed (attempt {attempt + 1}/"
                        f"{self.max_retries}): {error_msg}. Retrying in {delay}s..."
                    )
                    await asyncio.sleep(delay)
                else:
                    # Sanitize error message to avoid logging tokens
                    error_msg = str(e)
                    error_msg = re.sub(
                        r"(token|api_?key|password|secret)=[^&\s]+",
                        r"\1=***",
                        error_msg,
                        flags=re.IGNORECASE,
                    )
                    error_msg = re.sub(r"://[^:]+:[^@]+@", r"://***:***@", error_msg)
                    logger.error(
                        f"LLM request failed after {self.max_retries} attempts: {error_msg}"
                    )

        # All retries failed
        raise last_exception

    async def analyze_code_json(
        self,
        system_prompt: str,
        code: str,
        schema: Optional[Type[BaseModel]] = None,
        repo_id: Optional[str] = None,
        commit_sha: Optional[str] = None,
        analyzer: str = "unknown",
    ) -> Dict[str, Any]:
        """Analyze code and parse JSON response with fallback strategies.

        Implements 5 fallback strategies:
        1. Extract from markdown code fences
        2. Direct JSON parse
        3. Fix common errors (trailing commas, single quotes)
        4. Recover truncated JSON (add missing brackets)
        5. Fuzzy inference using schema (if provided)

        Args:
            system_prompt: System prompt (should request JSON output)
            code: Code to analyze
            schema: Optional Pydantic model for validation and fuzzy inference
            repo_id: Optional repository identifier
            commit_sha: Optional commit SHA (auto-detected if not provided)

        Returns:
            Parsed JSON dict

        Raises:
            ValueError: If all parsing strategies fail
        """
        # Retry up to 3 times with increasingly strict prompts
        for attempt in range(3):
            response = await self.analyze_code(
                system_prompt=system_prompt,
                code=code,
                repo_id=repo_id,
                commit_sha=commit_sha,
                analyzer=analyzer,
            )
            content = response.content

            # Strategy 1: Extract from markdown fences
            if "```json" in content or "```" in content:
                match = re.search(r"```(?:json)?\n(.*?)\n```", content, re.DOTALL)
                if match:
                    content = match.group(1).strip()

            # Strategy 2: Direct parse
            try:
                result = json.loads(content)
                if schema:
                    # Validate with Pydantic
                    validated = schema(**result)
                    return validated.model_dump()
                return result
            except json.JSONDecodeError:
                pass

            # Strategy 3: Fix common errors
            try:
                # Remove trailing commas before } or ]
                cleaned = re.sub(r",(\s*[}\]])", r"\1", content)
                # Replace single quotes with double quotes (naive)
                cleaned = cleaned.replace("'", '"')
                result = json.loads(cleaned)
                if schema:
                    validated = schema(**result)
                    return validated.model_dump()
                return result
            except (json.JSONDecodeError, Exception):
                pass

            # Strategy 4: Recover truncated JSON
            try:
                # Count braces
                open_braces = content.count("{")
                close_braces = content.count("}")
                open_brackets = content.count("[")
                close_brackets = content.count("]")

                recovered = content
                if open_braces > close_braces:
                    recovered += "}" * (open_braces - close_braces)
                if open_brackets > close_brackets:
                    recovered += "]" * (open_brackets - close_brackets)

                result = json.loads(recovered)
                if schema:
                    validated = schema(**result)
                    return validated.model_dump()
                return result
            except (json.JSONDecodeError, Exception):
                pass

            # Strategy 5: Fuzzy inference (last resort, attempt 2 only)
            if attempt == 2 and schema:
                try:
                    result = self._fuzzy_inference(content, schema)
                    if result:
                        return result
                except Exception as e:
                    logger.debug(f"Fuzzy inference failed: {e}")

            # Retry with stricter prompt
            if attempt < 2:
                system_prompt += (
                    "\n\nIMPORTANT: Return ONLY valid, well-formed JSON. "
                    "No explanations, no markdown fences."
                )

        raise ValueError(f"Failed to parse JSON after 3 attempts. Last content: {content[:200]}...")

    def _fuzzy_inference(self, content: str, schema: Type[BaseModel]) -> Optional[Dict[str, Any]]:
        """Attempt to extract data from malformed response using schema.

        Uses regex to extract values for expected fields.

        Args:
            content: Malformed response content
            schema: Pydantic model schema

        Returns:
            Extracted dict or None if extraction fails
        """
        # Get schema fields
        fields = schema.model_fields

        result = {}
        for field_name, field_info in fields.items():
            # Try to extract field value using various patterns
            patterns = [
                # "field_name": "value"
                rf'"{field_name}"\s*:\s*"([^"]*)"',
                # "field_name": value (number/boolean)
                rf'"{field_name}"\s*:\s*([^,\}}\]]+)',
                # field_name: "value"
                rf"{field_name}\s*:\s*\"([^\"]*)\"",
                # Natural language: "field_name is value"
                rf'{field_name}\s+is\s+"([^"]*)"',
                # Natural language: field_name is value (number)
                rf"{field_name}\s+is\s+(\d+)",
            ]

            for pattern in patterns:
                match = re.search(pattern, content, re.IGNORECASE)
                if match:
                    value = match.group(1).strip()
                    # Try to convert to appropriate type
                    if field_info.annotation is int:
                        try:
                            result[field_name] = int(value)
                        except ValueError:
                            pass
                    elif field_info.annotation is float:
                        try:
                            result[field_name] = float(value)
                        except ValueError:
                            pass
                    elif field_info.annotation is bool:
                        result[field_name] = value.lower() in ("true", "1", "yes")
                    else:
                        result[field_name] = value
                    break

        # Validate extracted data
        if result:
            try:
                validated = schema(**result)
                return validated.model_dump()
            except Exception:
                pass

        return None

    def get_metrics(self) -> Dict[str, Any]:
        """Get current metrics as dictionary.

        Returns:
            Dict with metrics from metrics object
        """
        return self.metrics.to_dict()

    def get_llm_metrics(self) -> LLMMetrics:
        """Get LLMMetrics object with detailed statistics.

        Returns:
            LLMMetrics object with comprehensive usage statistics
        """
        return self.metrics

    async def close(self):
        """Close the client and release resources."""
        # Close Bedrock client if using Bedrock provider
        if self._provider == "bedrock" and hasattr(self, "bedrock_client"):
            await self.bedrock_client.close()
            return

        # Prefer closing compat client to satisfy tests that patch client.close
        if hasattr(self, "client") and hasattr(self.client, "close"):
            try:
                await self.client.close()
                return
            except Exception:
                pass
        if hasattr(self, "http") and self.http:
            await self.http.aclose()
