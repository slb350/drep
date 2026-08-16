"""Rate limiting for LLM requests.

Multi-level rate limiting enforced via async context managers:

- Global concurrency limit (total in-flight requests)
- Per-repo concurrency limit (fair sharing across repositories)
- Requests-per-minute sliding window
- Tokens-per-minute token bucket with estimate/actual reconciliation

``RateLimitContext.__aenter__`` acquires the permits and reserves estimated
tokens; ``__aexit__`` reconciles actual usage and releases everything. If
entry is cancelled or a rate check raises, already-acquired permits are
rolled back explicitly (``__aexit__`` never runs in that case).
"""

import asyncio
import logging
import time
from collections import deque

from drep.constants import REPO_SEMAPHORE_TTL_SECONDS

logger = logging.getLogger(__name__)


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

    def __init__(self, rate_limiter: "RateLimiter", estimated_tokens: int, repo_id: str | None):
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
        self.actual_tokens: int | None = None  # Set by caller after request completes
        self.repo_semaphore: asyncio.Semaphore | None = None  # Set in __aenter__

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
        # STEP 1: Check request rate limit (requests per minute).
        # Sliding window: track timestamps of recent requests and wait if adding
        # this one would exceed the per-minute limit. This runs BEFORE the
        # concurrency permits are taken so a merely time-throttled request does
        # not occupy one of the scarce max_concurrent slots while it sleeps.
        await self.rate_limiter._check_request_rate_limit()

        # If entry is cancelled or a later step raises, __aexit__ never runs, so
        # any permit acquired below must be rolled back explicitly — otherwise
        # it leaks permanently (a leaked repo permit blocks that repo forever).
        acquired_global = False
        try:
            # STEP 2: Acquire global semaphore (limits total concurrent requests).
            # Blocks if max_concurrent_global requests are already in flight;
            # held until __aexit__ to ensure proper concurrency control.
            await self.rate_limiter.semaphore.acquire()
            acquired_global = True

            # STEP 3: Acquire per-repo semaphore if repo_id specified.
            # This prevents one repository from monopolizing all concurrent slots.
            # Example: If max_concurrent_global=5 and max_concurrent_per_repo=3,
            # then repo A can use at most 3 slots, leaving 2+ for other repos.
            if self.repo_id is not None:
                self.repo_semaphore = await self.rate_limiter._get_repo_semaphore(self.repo_id)
                await self.repo_semaphore.acquire()

            # STEP 4: Check token rate limit (tokens per minute).
            # Token bucket: track cumulative tokens in the current minute and wait
            # if adding the estimate would exceed the limit. Reserving last means
            # there is no await between reservation and return, so the reservation
            # can never be stranded by cancellation.
            await self.rate_limiter._check_token_rate_limit(self.estimated_tokens)
        except BaseException:
            self._release_permits(acquired_global)
            raise

        return self

    def _release_permits(self, acquired_global: bool = True) -> None:
        """Release the per-repo and global concurrency permits, in that order.

        Shared by the ``__aenter__`` rollback path and ``__aexit__`` so the
        release order stays identical in both.
        """
        if self.repo_semaphore is not None:
            self.repo_semaphore.release()
            self.repo_semaphore = None
        if acquired_global:
            self.rate_limiter.semaphore.release()

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
        # STEP 1: Reconcile token accounting under lock (to prevent race conditions).
        # The estimate is always released; the actual usage is charged on top when
        # the request got far enough to report it. Clamping to 0 handles the edge
        # case where the bucket was reset mid-request.
        async with self.rate_limiter.lock:
            self.rate_limiter.tokens_used = max(
                0, self.rate_limiter.tokens_used - self.estimated_tokens
            )
            if self.actual_tokens is not None:
                self.rate_limiter.tokens_used += self.actual_tokens
                logger.debug(
                    f"Token reconciliation: estimated={self.estimated_tokens}, "
                    f"actual={self.actual_tokens}, "
                    f"total={self.rate_limiter.tokens_used}"
                )
            else:
                # Request failed before set_actual_tokens() was called. Without the
                # rollback above, failed requests would permanently reduce the
                # available token budget.
                logger.debug(
                    f"Rolling back {self.estimated_tokens} token reservation "
                    f"(request failed without completion)"
                )

        # STEP 2: Release the concurrency permits (repo first, then global)
        self._release_permits()

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
        max_concurrent_per_repo: int | None = None,
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
        self.repo_semaphores: dict[str, asyncio.Semaphore] = {}

        # Track last access time for each repo's semaphore (for cleanup)
        # Maps repo_id -> timestamp (seconds since epoch)
        self.repo_last_used: dict[str, float] = {}

        # Time-to-live for idle repo semaphores: 10 minutes
        # After 10 minutes of inactivity, a repo's semaphore is eligible for eviction
        # This prevents memory leaks when scanning many repos over time
        self.repo_semaphore_ttl = REPO_SEMAPHORE_TTL_SECONDS
        # Timestamp of the last idle-semaphore eviction sweep (see
        # _get_repo_semaphore); throttles the sweep to once per TTL interval.
        self._last_sweep = time.time()

        # Request rate limiting: Sliding window algorithm
        # Lock protects all shared state from concurrent access
        self.lock = asyncio.Lock()

        # List of request timestamps (seconds since epoch) in the last minute
        # This list is continuously pruned to remove timestamps >60s old
        # Length of list = number of requests in last 60 seconds
        # deque so pruning the sliding window is O(expired) popleft() calls
        # instead of rebuilding the whole list on every request.
        self.request_times: deque[float] = deque()

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

            # CLEANUP PHASE: Identify and evict idle semaphores.
            # The sweep is O(tracked repos), so it runs at most once per TTL
            # interval rather than on every single request.
            idle_repos: list[str] = []
            if now - self._last_sweep >= self.repo_semaphore_ttl:
                self._last_sweep = now
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
        # The lock is released before every sleep: holding it across an await
        # would block RateLimitContext.__aexit__ (which needs the same lock to
        # reconcile tokens before releasing its permits), stalling the pipeline
        # for the whole wait.
        while True:
            async with self.lock:  # Protect request_times from concurrent access
                now = time.time()

                # PRUNE PHASE: Drop timestamps older than 60 seconds, maintaining
                # a sliding 60-second window.
                while self.request_times and now - self.request_times[0] >= 60:
                    self.request_times.popleft()

                if len(self.request_times) < self.requests_per_minute:
                    # RECORD PHASE: Window has room - claim a slot and return.
                    self.request_times.append(now)
                    return

                # Window is full - wait for the oldest request to age out.
                wait_time = 60 - (now - self.request_times[0])

            if wait_time > 0:
                logger.debug(f"Request rate limit reached, waiting {wait_time:.1f}s")
                await asyncio.sleep(wait_time)

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
        # As in _check_request_rate_limit, the lock is never held across a sleep.
        while True:
            async with self.lock:  # Protect shared token counters
                now = time.time()

                # RESET PHASE: Check if current minute has expired
                if now >= self.token_reset_time:
                    # Minute boundary crossed - reset counter and update reset time
                    self.tokens_used = 0
                    self.token_reset_time = now + 60
                    logger.debug(f"Token bucket reset at {now}")

                if self.tokens_used + estimated_tokens <= self.max_tokens_per_minute:
                    # RESERVATION PHASE: Reserve estimated tokens for this request.
                    # This prevents token limit bypass when multiple requests are
                    # queued. Actual tokens are reconciled in __aexit__.
                    self.tokens_used += estimated_tokens
                    logger.debug(
                        f"Reserved {estimated_tokens} tokens "
                        f"(total: {self.tokens_used}/{self.max_tokens_per_minute})"
                    )
                    return

                # Bucket is full - wait for the next minute boundary.
                wait_time = self.token_reset_time - now
                logger.debug(
                    f"Token rate limit reached "
                    f"({self.tokens_used}/{self.max_tokens_per_minute}), "
                    f"waiting {wait_time:.1f}s"
                )

            if wait_time > 0:
                await asyncio.sleep(wait_time)

    def request(self, estimated_tokens: int, repo_id: str | None = None):
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
        # A request larger than the entire per-minute budget can never fit, and
        # would otherwise spin forever in _check_token_rate_limit. Clamp the
        # reservation so it proceeds once the bucket is empty. Clamping here (not
        # inside the check) keeps the reservation and the __aexit__ rollback in
        # sync, since both use RateLimitContext.estimated_tokens.
        if estimated_tokens > self.max_tokens_per_minute:
            logger.warning(
                f"Estimated tokens ({estimated_tokens}) exceed the per-minute budget "
                f"({self.max_tokens_per_minute}); clamping the reservation to the budget"
            )
            estimated_tokens = self.max_tokens_per_minute

        return RateLimitContext(self, estimated_tokens, repo_id)
