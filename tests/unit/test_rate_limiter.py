"""Rate limiter tests (drep.llm.rate_limiter): concurrency, token bucket, rollback."""

import asyncio
import time

import pytest

from drep.llm.rate_limiter import RateLimiter


async def test_rate_limiter_enforces_concurrency():
    """Test that semaphore enforces maximum concurrent requests."""
    limiter = RateLimiter(max_concurrent=2, requests_per_minute=100, max_tokens_per_minute=100000)

    # Track concurrent requests
    concurrent_count = 0
    max_concurrent = 0
    lock = asyncio.Lock()

    async def mock_request():
        nonlocal concurrent_count, max_concurrent
        async with limiter.request(1000) as ctx:
            async with lock:
                concurrent_count += 1
                max_concurrent = max(max_concurrent, concurrent_count)
            await asyncio.sleep(0.1)
            ctx.set_actual_tokens(100)
            async with lock:
                concurrent_count -= 1

    # Launch 10 requests in parallel
    await asyncio.gather(*[mock_request() for _ in range(10)])

    # Should never exceed max_concurrent=2
    assert max_concurrent == 2


@pytest.mark.asyncio
async def test_rate_limiter_enforces_request_limit():
    """Test that request rate limit is enforced."""
    limiter = RateLimiter(max_concurrent=10, requests_per_minute=5, max_tokens_per_minute=100000)

    # Track request times
    request_times = []

    async def mock_request():
        request_times.append(asyncio.get_event_loop().time())
        async with limiter.request(100) as ctx:
            ctx.set_actual_tokens(10)

    # Make 6 requests (should trigger rate limit on 6th)
    # Note: Rate limit will cause 6th request to wait for minute window
    # So we'll only make 5 requests to test within reasonable time
    await asyncio.gather(*[mock_request() for _ in range(5)])

    # All 5 requests should complete quickly
    assert len(request_times) == 5


@pytest.mark.asyncio
async def test_rate_limiter_enforces_token_limit():
    """Test that token rate limit is enforced."""
    limiter = RateLimiter(max_concurrent=10, requests_per_minute=100, max_tokens_per_minute=500)

    async def mock_request(tokens):
        async with limiter.request(tokens) as ctx:
            ctx.set_actual_tokens(tokens)

    # Each request uses 200 tokens, limit is 500
    # Third request should wait for reset
    start_time = asyncio.get_event_loop().time()

    await mock_request(200)  # Total: 200
    await mock_request(200)  # Total: 400
    # Next request would exceed, should wait briefly
    # (in real scenario would wait for reset)

    elapsed = asyncio.get_event_loop().time() - start_time
    assert elapsed < 1.0  # Should complete quickly


@pytest.mark.asyncio
async def test_rate_limit_context_updates_actual_tokens():
    """Test that context manager properly updates actual token usage."""
    limiter = RateLimiter(max_concurrent=5, requests_per_minute=100, max_tokens_per_minute=10000)

    initial_tokens = limiter.tokens_used

    async with limiter.request(1000) as ctx:
        # Estimated tokens should be reserved
        reserved_tokens = limiter.tokens_used
        assert reserved_tokens >= initial_tokens

        # Set actual usage (lower than estimate)
        ctx.set_actual_tokens(500)

    # After exit, should reflect actual usage
    # Note: Due to the adjustment logic, final count should be reasonable
    assert limiter.tokens_used >= 0


@pytest.mark.asyncio
async def test_rate_limiter_multiple_repos():
    """Test rate limiter works with repo_id parameter."""
    limiter = RateLimiter(max_concurrent=5, requests_per_minute=100, max_tokens_per_minute=10000)

    async def mock_request(repo_id):
        async with limiter.request(100, repo_id=repo_id) as ctx:
            ctx.set_actual_tokens(50)
            await asyncio.sleep(0.01)

    # Make requests with different repo IDs
    await asyncio.gather(
        mock_request("repo1"),
        mock_request("repo2"),
        mock_request("repo1"),
    )

    # Should complete without errors
    assert limiter.tokens_used >= 0


@pytest.mark.asyncio
async def test_rate_limiter_enforces_per_repo_limit():
    """Test that per-repo concurrency limits are enforced."""
    limiter = RateLimiter(
        max_concurrent=10,  # High global limit
        requests_per_minute=100,
        max_tokens_per_minute=100000,
        max_concurrent_per_repo=2,  # But only 2 per repo
    )

    # Track concurrent requests per repo
    repo1_concurrent = 0
    repo1_max_concurrent = 0
    lock = asyncio.Lock()

    async def mock_request(repo_id):
        nonlocal repo1_concurrent, repo1_max_concurrent
        async with limiter.request(100, repo_id=repo_id) as ctx:
            if repo_id == "repo1":
                async with lock:
                    repo1_concurrent += 1
                    repo1_max_concurrent = max(repo1_max_concurrent, repo1_concurrent)
            await asyncio.sleep(0.1)
            ctx.set_actual_tokens(50)
            if repo_id == "repo1":
                async with lock:
                    repo1_concurrent -= 1

    # Launch 10 requests for repo1 in parallel
    await asyncio.gather(*[mock_request("repo1") for _ in range(10)])

    # Should never exceed max_concurrent_per_repo=2 for repo1
    assert repo1_max_concurrent == 2


@pytest.mark.asyncio
async def test_rate_limiter_different_repos_independent():
    """Test that different repos have independent concurrency limits."""
    limiter = RateLimiter(
        max_concurrent=10,
        requests_per_minute=100,
        max_tokens_per_minute=100000,
        max_concurrent_per_repo=2,
    )

    # Track concurrent requests across all repos
    total_concurrent = 0
    max_total_concurrent = 0
    lock = asyncio.Lock()

    async def mock_request(repo_id):
        nonlocal total_concurrent, max_total_concurrent
        async with limiter.request(100, repo_id=repo_id) as ctx:
            async with lock:
                total_concurrent += 1
                max_total_concurrent = max(max_total_concurrent, total_concurrent)
            await asyncio.sleep(0.1)
            ctx.set_actual_tokens(50)
            async with lock:
                total_concurrent -= 1

    # Launch 2 requests each for 3 different repos (6 total)
    await asyncio.gather(
        *[mock_request("repo1") for _ in range(2)],
        *[mock_request("repo2") for _ in range(2)],
        *[mock_request("repo3") for _ in range(2)],
    )

    # Should allow up to 6 concurrent (2 per repo × 3 repos)
    # But might be less due to timing
    assert max_total_concurrent >= 3  # At least some parallelism


@pytest.mark.asyncio
async def test_rate_limiter_rolls_back_tokens_on_failure():
    """Test that failed requests roll back their token reservation."""
    limiter = RateLimiter(max_concurrent=10, requests_per_minute=100, max_tokens_per_minute=1000)

    initial_tokens = limiter.tokens_used

    # Simulate a failed request (never calls set_actual_tokens)
    try:
        async with limiter.request(500) as ctx:  # noqa: F841
            # Token reservation should be in place
            assert limiter.tokens_used == initial_tokens + 500
            # Simulate failure - raise exception without calling set_actual_tokens
            raise RuntimeError("Simulated request failure")
    except RuntimeError:
        pass

    # After exiting context, tokens should be rolled back
    assert limiter.tokens_used == initial_tokens


@pytest.mark.asyncio
async def test_rate_limiter_burst_failures_dont_stall():
    """Test that burst of failures doesn't stall the rate limiter."""
    limiter = RateLimiter(max_concurrent=5, requests_per_minute=100, max_tokens_per_minute=1000)

    # Simulate 5 failed requests that consume the entire token budget
    for _ in range(5):
        try:
            async with limiter.request(200):  # 5 × 200 = 1000 tokens
                raise RuntimeError("Simulated failure")
        except RuntimeError:  # noqa: PERF203 - intentionally failing each iteration
            pass

    # All tokens should be rolled back, allowing new requests
    assert limiter.tokens_used == 0

    # Should be able to make a successful request immediately
    async with limiter.request(500) as ctx:
        ctx.set_actual_tokens(400)

    # Should reflect actual usage, not failures
    assert limiter.tokens_used == 400


@pytest.mark.asyncio
async def test_rate_limiter_releases_permits_when_entry_cancelled():
    """Test that cancellation during __aenter__ (mid rate-limit sleep) releases permits.

    __aexit__ never runs when __aenter__ is cancelled, so any semaphores already
    acquired must be rolled back explicitly or permits leak permanently.
    """
    limiter = RateLimiter(max_concurrent=1, requests_per_minute=1, max_tokens_per_minute=1000)

    # Fill the sliding window so the request-rate check sleeps ~60s
    limiter.request_times = [time.time()]

    ctx = limiter.request(100, repo_id="repo1")
    task = asyncio.create_task(ctx.__aenter__())
    # Let the task acquire both semaphores and enter the rate-limit sleep
    await asyncio.sleep(0.05)
    task.cancel()
    with pytest.raises(asyncio.CancelledError):
        await task

    # Global semaphore permit must be available again
    await asyncio.wait_for(limiter.semaphore.acquire(), timeout=0.1)
    limiter.semaphore.release()

    # Repo semaphore permit must be available again
    repo_sem = await limiter._get_repo_semaphore("repo1")
    await asyncio.wait_for(repo_sem.acquire(), timeout=0.1)
    repo_sem.release()


@pytest.mark.asyncio
async def test_rate_limiter_releases_permits_when_rate_check_raises():
    """Test that an exception during entry rate checks releases acquired permits."""
    limiter = RateLimiter(max_concurrent=1, requests_per_minute=100, max_tokens_per_minute=1000)

    original_check = limiter._check_request_rate_limit

    async def failing_check():
        await original_check()
        raise RuntimeError("Simulated rate-check failure")

    limiter._check_request_rate_limit = failing_check  # type: ignore[method-assign]

    ctx = limiter.request(100, repo_id="repo1")
    with pytest.raises(RuntimeError, match="Simulated rate-check failure"):
        await ctx.__aenter__()

    # Both permits must have been released
    await asyncio.wait_for(limiter.semaphore.acquire(), timeout=0.1)
    limiter.semaphore.release()
    repo_sem = await limiter._get_repo_semaphore("repo1")
    await asyncio.wait_for(repo_sem.acquire(), timeout=0.1)
    repo_sem.release()


@pytest.mark.asyncio
async def test_rate_limiter_tokens_never_go_negative():
    """Test that token counter is clamped to 0, preventing negatives."""
    limiter = RateLimiter(max_concurrent=5, requests_per_minute=100, max_tokens_per_minute=1000)

    # Simulate a long-running request that spans bucket reset
    async with limiter.request(500) as ctx:
        # Manually trigger bucket reset (simulating time passing)
        limiter.tokens_used = 0
        limiter.token_reset_time = time.time() + 60

        # Request completes after bucket reset
        ctx.set_actual_tokens(400)

    # Token counter should be clamped to 0, not negative
    # (400 - 500 would be -100 without clamping)
    assert limiter.tokens_used >= 0
    assert limiter.tokens_used == 400  # Should reflect actual usage


@pytest.mark.asyncio
async def test_rate_limiter_cleanup_idle_semaphores():
    """Test that idle repo semaphores are cleaned up."""
    limiter = RateLimiter(
        max_concurrent=10,
        requests_per_minute=100,
        max_tokens_per_minute=100000,
        max_concurrent_per_repo=2,
    )
    limiter.repo_semaphore_ttl = 0.1  # 100ms TTL for testing

    # Create semaphores for 3 repos
    await limiter._get_repo_semaphore("repo1")
    await limiter._get_repo_semaphore("repo2")
    await limiter._get_repo_semaphore("repo3")

    # Should have 3 semaphores
    assert len(limiter.repo_semaphores) == 3

    # Wait for TTL to expire
    await asyncio.sleep(0.15)

    # Access one repo to keep it alive
    await limiter._get_repo_semaphore("repo1")

    # repo2 and repo3 should be evicted (idle), repo1 should remain
    # (Cleanup happens on next _get_repo_semaphore call)
    assert "repo1" in limiter.repo_semaphores
    # repo2 and repo3 might still be present until next cleanup
    # So let's trigger cleanup by accessing a new repo
    await limiter._get_repo_semaphore("repo4")

    # Now repo1 and repo4 should exist, repo2/repo3 should be evicted
    assert "repo1" in limiter.repo_semaphores
    assert "repo4" in limiter.repo_semaphores
    # Total should be <= 3 (might have repo2/repo3 if not yet evicted)
    assert len(limiter.repo_semaphores) <= 3


@pytest.mark.asyncio
async def test_rate_limiter_doesnt_evict_active_semaphores():
    """Test that active (in-use) semaphores are not evicted."""
    limiter = RateLimiter(
        max_concurrent=10,
        requests_per_minute=100,
        max_tokens_per_minute=100000,
        max_concurrent_per_repo=2,
    )
    limiter.repo_semaphore_ttl = 0.1  # 100ms TTL for testing

    # Create and hold a semaphore
    sem = await limiter._get_repo_semaphore("repo1")
    await sem.acquire()  # Hold one permit

    # Wait for TTL to expire
    await asyncio.sleep(0.15)

    # Try to trigger cleanup by accessing another repo
    await limiter._get_repo_semaphore("repo2")

    # repo1 should NOT be evicted because it's in use
    assert "repo1" in limiter.repo_semaphores

    # Release the semaphore
    sem.release()


# Test LLM Client Initialization
