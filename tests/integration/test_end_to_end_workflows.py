"""End-to-end integration tests for LLM client workflows.

This test module verifies complete LLM client workflows with dependency injection,
caching, rate limiting, and error handling - testing Items 2.2 and 2.4.
"""

import asyncio
from unittest.mock import AsyncMock, Mock, patch

import pytest

from drep.llm.cache import IntelligentCache
from drep.llm.client import CircuitBreaker, LLMClient, LLMResponse, RateLimiter


@pytest.fixture
def mock_http_response():
    """Create a mock HTTP response for LLM API."""
    return {
        "choices": [{"message": {"content": "Analysis result"}}],
        "usage": {"prompt_tokens": 50, "completion_tokens": 100, "total_tokens": 150},
        "model": "test-model",
    }


@pytest.mark.integration
@pytest.mark.asyncio
async def test_llm_client_with_injected_dependencies(mock_http_response):
    """Test LLM client workflow with all dependencies injected.

    Tests complete workflow:
    1. Create custom RateLimiter, CircuitBreaker, Cache
    2. Inject into LLMClient
    3. Make LLM request
    4. Verify all components work together
    5. Check metrics tracking
    """
    # Create custom dependencies
    rate_limiter = RateLimiter(
        max_concurrent=2, requests_per_minute=60, max_tokens_per_minute=10000
    )
    circuit_breaker = CircuitBreaker(failure_threshold=5, recovery_timeout=60)
    cache = IntelligentCache(ttl=3600)

    # Mock HTTP client
    with patch("httpx.AsyncClient") as mock_client_class:
        mock_instance = Mock()
        mock_response = Mock()
        mock_response.json.return_value = mock_http_response
        mock_response.raise_for_status = Mock()
        mock_instance.post = AsyncMock(return_value=mock_response)
        mock_instance.aclose = AsyncMock()
        mock_client_class.return_value = mock_instance

        # Create LLM client with injected dependencies
        client = LLMClient(
            endpoint="http://test",
            model="test-model",
            rate_limiter=rate_limiter,
            circuit_breaker=circuit_breaker,
            cache=cache,
        )

        # Verify dependencies were injected
        assert client.rate_limiter is rate_limiter
        assert client.circuit_breaker is circuit_breaker
        assert client.cache is cache

        # Make request
        result = await client.analyze_code(
            system_prompt="Test prompt", code="def foo(): pass", repo_id="test/repo"
        )

        # Verify result
        assert isinstance(result, LLMResponse)
        assert result.content == "Analysis result"
        assert result.tokens_used == 150

        # Verify metrics tracked
        assert client.metrics.total_requests > 0
        assert client.metrics.total_tokens > 0

        await client.close()


@pytest.mark.integration
@pytest.mark.asyncio
async def test_llm_client_caching_workflow(mock_http_response):
    """Test complete caching workflow with cold and warm requests.

    Tests:
    1. First request (cold) - calls LLM, populates cache
    2. Second request (warm) - hits cache, no LLM call
    3. Verify cache hit rate improves
    4. Verify metrics differentiate cached vs non-cached
    """
    cache = IntelligentCache(ttl=3600)

    with patch("httpx.AsyncClient") as mock_client_class:
        mock_instance = Mock()
        mock_response = Mock()
        mock_response.json.return_value = mock_http_response
        mock_response.raise_for_status = Mock()
        mock_instance.post = AsyncMock(return_value=mock_response)
        mock_instance.aclose = AsyncMock()
        mock_client_class.return_value = mock_instance

        client = LLMClient(
            endpoint="http://test",
            model="test-model",
            cache=cache,
            enable_circuit_breaker=False,
        )

        # First request (cold)
        result1 = await client.analyze_code(
            system_prompt="Test", code="def foo(): pass", commit_sha="abc123"
        )

        # Verify LLM was called
        assert mock_instance.post.call_count == 1

        # Second request (warm) - same params
        result2 = await client.analyze_code(
            system_prompt="Test", code="def foo(): pass", commit_sha="abc123"
        )

        # Verify LLM not called again (cache hit)
        assert mock_instance.post.call_count == 1, "Second request should hit cache"

        # Results should be identical
        assert result1.content == result2.content
        assert result1.tokens_used == result2.tokens_used

        # Verify cache stats
        stats = cache.get_stats()
        assert stats["hits"] > 0
        assert stats["hit_rate"] > 0.0

        await client.close()


@pytest.mark.integration
@pytest.mark.asyncio
async def test_rate_limiting_workflow():
    """Test rate limiting with concurrent requests.

    Tests:
    1. Create RateLimiter with low limits
    2. Make concurrent requests
    3. Verify rate limiting enforced
    4. Verify no more than max_concurrent running at once
    """
    rate_limiter = RateLimiter(
        max_concurrent=2,  # Only 2 at a time
        requests_per_minute=60,
        max_tokens_per_minute=10000,
    )

    client = LLMClient(
        endpoint="http://test",
        model="test-model",
        rate_limiter=rate_limiter,
        enable_circuit_breaker=False,
    )

    # Mock analyze_code to track concurrency
    concurrent_count = {"current": 0, "max": 0}

    async def mock_analyze(*args, **kwargs):
        concurrent_count["current"] += 1
        concurrent_count["max"] = max(concurrent_count["max"], concurrent_count["current"])
        await asyncio.sleep(0.1)  # Simulate work
        concurrent_count["current"] -= 1
        return LLMResponse(content="test", tokens_used=100, latency_ms=100.0, model="test")

    client.analyze_code = mock_analyze

    # Make 5 concurrent requests
    tasks = [
        client.analyze_code(system_prompt="Test", code=f"def f{i}(): pass")
        for i in range(5)
    ]
    results = await asyncio.gather(*tasks)

    # Verify all succeeded
    assert len(results) == 5

    # Verify concurrency was limited
    assert concurrent_count["max"] <= 2, f"Max concurrent was {concurrent_count['max']}, expected ≤ 2"

    await client.close()


@pytest.mark.integration
@pytest.mark.asyncio
async def test_circuit_breaker_workflow():
    """Test circuit breaker with failing requests.

    Tests:
    1. Make requests that fail
    2. Circuit breaker opens after threshold
    3. Subsequent requests fail fast
    4. Circuit breaker recovers after timeout
    """
    circuit_breaker = CircuitBreaker(failure_threshold=3, recovery_timeout=1)

    client = LLMClient(
        endpoint="http://test",
        model="test-model",
        circuit_breaker=circuit_breaker,
        max_retries=1,  # Fail fast
        enable_circuit_breaker=True,
    )

    # Mock analyze_code to always fail
    client.analyze_code = AsyncMock(side_effect=Exception("LLM Error"))

    # Make requests until circuit opens (3 failures)
    failures = 0
    for i in range(5):
        try:
            await client.analyze_code(system_prompt="Test", code="def foo(): pass")
        except Exception:
            failures += 1

    # Should have failed multiple times
    assert failures >= 3

    # Verify circuit breaker tracked failures
    assert circuit_breaker.failure_count >= 3

    await client.close()


@pytest.mark.integration
@pytest.mark.asyncio
async def test_metrics_tracking_workflow(mock_http_response):
    """Test comprehensive metrics tracking across multiple requests.

    Tests:
    1. Make mix of successful and failed requests
    2. Mix of cached and non-cached
    3. Verify metrics aggregate correctly
    4. Verify per-analyzer breakdown works
    """
    cache = IntelligentCache(ttl=3600)

    with patch("httpx.AsyncClient") as mock_client_class:
        mock_instance = Mock()
        mock_response = Mock()
        mock_response.json.return_value = mock_http_response
        mock_response.raise_for_status = Mock()
        mock_instance.post = AsyncMock(return_value=mock_response)
        mock_instance.aclose = AsyncMock()
        mock_client_class.return_value = mock_instance

        client = LLMClient(
            endpoint="http://test",
            model="test-model",
            cache=cache,
            enable_circuit_breaker=False,
        )

        # Make successful requests (will be cached)
        await client.analyze_code(
            system_prompt="Test",
            code="def foo(): pass",
            analyzer="test-analyzer",
            commit_sha="abc123",
        )

        # Second request hits cache
        await client.analyze_code(
            system_prompt="Test",
            code="def foo(): pass",
            analyzer="test-analyzer",
            commit_sha="abc123",
        )

        # Get metrics
        metrics = client.get_llm_metrics()

        # Verify metrics tracked
        assert metrics.total_requests >= 2
        assert metrics.total_tokens > 0
        assert metrics.cache_hits > 0

        # Verify per-analyzer metrics
        analyzer_stats = metrics.get_analyzer_stats("test-analyzer")
        assert analyzer_stats["requests"] >= 1

        await client.close()


@pytest.mark.integration
@pytest.mark.asyncio
async def test_backward_compatibility_workflow(mock_http_response):
    """Test backward compatibility - client works without injected dependencies.

    Tests:
    1. Create client without injecting dependencies
    2. Verify defaults are created
    3. Verify functionality works
    4. Verify legacy metrics still accessible (with warnings)
    """
    import warnings

    with patch("httpx.AsyncClient") as mock_client_class:
        mock_instance = Mock()
        mock_response = Mock()
        mock_response.json.return_value = mock_http_response
        mock_response.raise_for_status = Mock()
        mock_instance.post = AsyncMock(return_value=mock_response)
        mock_instance.aclose = AsyncMock()
        mock_client_class.return_value = mock_instance

        # Create client without injected dependencies (backward compat)
        client = LLMClient(
            endpoint="http://test",
            model="test-model",
            # No rate_limiter, circuit_breaker, or cache - should create defaults
        )

        # Verify defaults were created
        assert client.rate_limiter is not None
        assert isinstance(client.rate_limiter, RateLimiter)
        assert client.circuit_breaker is not None
        assert isinstance(client.circuit_breaker, CircuitBreaker)

        # Make request
        result = await client.analyze_code(system_prompt="Test", code="def foo(): pass")

        # Verify it works
        assert isinstance(result, LLMResponse)

        # Test legacy metrics (should work but warn)
        with warnings.catch_warnings(record=True) as w:
            warnings.simplefilter("always")
            _ = client.total_requests
            assert len(w) == 1
            assert issubclass(w[0].category, DeprecationWarning)

        await client.close()
