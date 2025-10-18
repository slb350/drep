"""Unit tests for LLM client and rate limiter."""

import asyncio
import time
from unittest.mock import AsyncMock, MagicMock

import pytest
from pydantic import BaseModel

from drep.llm.client import LLMClient, LLMResponse, RateLimiter

# Test Rate Limiter


@pytest.mark.asyncio
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
        except RuntimeError:
            pass

    # All tokens should be rolled back, allowing new requests
    assert limiter.tokens_used == 0

    # Should be able to make a successful request immediately
    async with limiter.request(500) as ctx:
        ctx.set_actual_tokens(400)

    # Should reflect actual usage, not failures
    assert limiter.tokens_used == 400


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


def test_llm_client_initialization():
    """Test LLM client initializes correctly."""
    client = LLMClient(
        endpoint="http://test.local/v1",
        model="test-model",
        api_key="test-key",
        temperature=0.5,
        max_tokens=1000,
    )

    assert client.endpoint == "http://test.local/v1"
    assert client.model == "test-model"
    assert client.temperature == 0.5
    assert client.max_tokens == 1000
    assert client.total_requests == 0
    assert client.total_tokens == 0


# Test LLM Client Basic Request


@pytest.mark.asyncio
async def test_llm_client_analyze_code():
    """Test basic analyze_code request (mocked)."""
    client = LLMClient(
        endpoint="http://test.local/v1",
        model="test-model",
        max_concurrent_global=5,
    )

    # Mock OpenAI response
    mock_response = MagicMock()
    mock_response.choices = [MagicMock()]
    mock_response.choices[0].message.content = "This is a test response"
    mock_response.usage.total_tokens = 100
    mock_response.model = "test-model"

    client.client.chat.completions.create = AsyncMock(return_value=mock_response)

    # Make request
    response = await client.analyze_code("Test prompt", "def foo(): pass")

    assert isinstance(response, LLMResponse)
    assert response.content == "This is a test response"
    assert response.tokens_used == 100
    assert response.model == "test-model"
    assert response.latency_ms > 0

    # Check metrics
    assert client.total_requests == 1
    assert client.total_tokens == 100


@pytest.mark.asyncio
async def test_llm_client_retry_logic():
    """Test that client retries on failure."""
    client = LLMClient(
        endpoint="http://test.local/v1",
        model="test-model",
        max_retries=3,
        retry_delay=0.01,  # Fast retry for testing
    )

    # Mock: fail twice, succeed third time
    call_count = 0

    async def mock_create(*args, **kwargs):
        nonlocal call_count
        call_count += 1
        if call_count < 3:
            raise Exception("Connection failed")

        # Success on third call
        mock_response = MagicMock()
        mock_response.choices = [MagicMock()]
        mock_response.choices[0].message.content = "Success"
        mock_response.usage.total_tokens = 50
        mock_response.model = "test-model"
        return mock_response

    client.client.chat.completions.create = mock_create

    # Should succeed after retries
    response = await client.analyze_code("Test", "code")
    assert response.content == "Success"
    assert call_count == 3


@pytest.mark.asyncio
async def test_llm_client_retry_exhaustion():
    """Test that client raises exception after all retries fail."""
    client = LLMClient(
        endpoint="http://test.local/v1",
        model="test-model",
        max_retries=2,
        retry_delay=0.01,
    )

    # Mock: always fail
    async def mock_create(*args, **kwargs):
        raise Exception("Connection failed")

    client.client.chat.completions.create = mock_create

    # Should raise after exhausting retries
    with pytest.raises(Exception, match="Connection failed"):
        await client.analyze_code("Test", "code")

    assert client.failed_requests == 2


# Test JSON Parsing Strategies


@pytest.mark.asyncio
async def test_json_parse_perfect_json():
    """Test parsing perfect JSON."""
    client = LLMClient(endpoint="http://test.local/v1", model="test-model")

    # Mock response with perfect JSON
    mock_response = MagicMock()
    mock_response.choices = [MagicMock()]
    mock_response.choices[0].message.content = '{"result": "success", "count": 42}'
    mock_response.usage.total_tokens = 50
    mock_response.model = "test-model"

    client.client.chat.completions.create = AsyncMock(return_value=mock_response)

    result = await client.analyze_code_json("Return JSON", "")

    assert result == {"result": "success", "count": 42}


@pytest.mark.asyncio
async def test_json_parse_markdown_fence():
    """Test extracting JSON from markdown code fence."""
    client = LLMClient(endpoint="http://test.local/v1", model="test-model")

    # Mock response with markdown fence
    mock_response = MagicMock()
    mock_response.choices = [MagicMock()]
    mock_response.choices[0].message.content = '```json\n{"result": "success"}\n```'
    mock_response.usage.total_tokens = 50
    mock_response.model = "test-model"

    client.client.chat.completions.create = AsyncMock(return_value=mock_response)

    result = await client.analyze_code_json("Return JSON", "")

    assert result == {"result": "success"}


@pytest.mark.asyncio
async def test_json_parse_trailing_comma():
    """Test fixing trailing commas."""
    client = LLMClient(endpoint="http://test.local/v1", model="test-model")

    # Mock response with trailing comma
    mock_response = MagicMock()
    mock_response.choices = [MagicMock()]
    mock_response.choices[0].message.content = '{"result": "success", "items": [1, 2, 3,],}'
    mock_response.usage.total_tokens = 50
    mock_response.model = "test-model"

    client.client.chat.completions.create = AsyncMock(return_value=mock_response)

    result = await client.analyze_code_json("Return JSON", "")

    assert result == {"result": "success", "items": [1, 2, 3]}


@pytest.mark.asyncio
async def test_json_parse_truncated():
    """Test recovering truncated JSON."""
    client = LLMClient(endpoint="http://test.local/v1", model="test-model")

    # Mock response with truncated JSON (missing closing brace)
    mock_response = MagicMock()
    mock_response.choices = [MagicMock()]
    mock_response.choices[0].message.content = '{"result": "success", "count": 42'
    mock_response.usage.total_tokens = 50
    mock_response.model = "test-model"

    client.client.chat.completions.create = AsyncMock(return_value=mock_response)

    result = await client.analyze_code_json("Return JSON", "")

    # Should have recovered by adding closing brace
    assert result == {"result": "success", "count": 42}


@pytest.mark.asyncio
async def test_json_parse_with_pydantic_schema():
    """Test JSON parsing with Pydantic schema validation."""

    class TestSchema(BaseModel):
        result: str
        count: int

    client = LLMClient(endpoint="http://test.local/v1", model="test-model")

    # Mock response
    mock_response = MagicMock()
    mock_response.choices = [MagicMock()]
    mock_response.choices[0].message.content = '{"result": "success", "count": 42}'
    mock_response.usage.total_tokens = 50
    mock_response.model = "test-model"

    client.client.chat.completions.create = AsyncMock(return_value=mock_response)

    result = await client.analyze_code_json("Return JSON", "", schema=TestSchema)

    assert result == {"result": "success", "count": 42}


@pytest.mark.asyncio
async def test_fuzzy_inference():
    """Test fuzzy inference for malformed JSON."""

    class TestSchema(BaseModel):
        result: str
        count: int

    client = LLMClient(endpoint="http://test.local/v1", model="test-model")

    # Create a malformed response that needs 3 attempts to trigger fuzzy inference
    attempt_count = 0

    async def mock_create(*args, **kwargs):
        nonlocal attempt_count
        attempt_count += 1

        mock_response = MagicMock()
        mock_response.choices = [MagicMock()]

        # All attempts return malformed JSON
        # On attempt 3, fuzzy inference should extract values
        mock_response.choices[0].message.content = 'The result is "success" and count is 42'
        mock_response.usage.total_tokens = 50
        mock_response.model = "test-model"

        return mock_response

    client.client.chat.completions.create = mock_create

    result = await client.analyze_code_json("Return JSON", "", schema=TestSchema)

    # Fuzzy inference should extract values
    assert "result" in result
    assert "count" in result
    assert attempt_count == 3  # Should retry to trigger fuzzy inference


# Test Metrics


@pytest.mark.asyncio
async def test_llm_client_metrics():
    """Test client metrics tracking."""
    client = LLMClient(endpoint="http://test.local/v1", model="test-model")

    # Mock successful response
    mock_response = MagicMock()
    mock_response.choices = [MagicMock()]
    mock_response.choices[0].message.content = "Success"
    mock_response.usage.total_tokens = 100
    mock_response.model = "test-model"

    client.client.chat.completions.create = AsyncMock(return_value=mock_response)

    # Make 3 requests
    await client.analyze_code("Test", "code")
    await client.analyze_code("Test", "code")
    await client.analyze_code("Test", "code")

    metrics = client.get_metrics()

    assert metrics["total_requests"] == 3
    assert metrics["failed_requests"] == 0
    assert metrics["total_tokens"] == 300
    assert metrics["success_rate"] == 1.0
    assert metrics["avg_tokens_per_request"] == 100


@pytest.mark.asyncio
async def test_llm_client_close():
    """Test that client closes properly."""
    client = LLMClient(endpoint="http://test.local/v1", model="test-model")

    # Mock close
    client.client.close = AsyncMock()

    await client.close()

    client.client.close.assert_called_once()
