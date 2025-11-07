"""Tests for LLM client dependency injection.

This test module verifies that LLMClient supports dependency injection
for RateLimiter and CircuitBreaker, improving testability and flexibility.
"""

from drep.llm.circuit_breaker import CircuitBreaker
from drep.llm.client import LLMClient, RateLimiter


class MockRateLimiter(RateLimiter):
    """Mock RateLimiter for testing dependency injection."""

    def __init__(self):
        """Initialize with minimal configuration."""
        super().__init__(
            max_concurrent=1,
            requests_per_minute=10,
            max_tokens_per_minute=1000,
        )
        self.was_injected = True


class MockCircuitBreaker(CircuitBreaker):
    """Mock CircuitBreaker for testing dependency injection."""

    def __init__(self):
        """Initialize with minimal configuration."""
        super().__init__(failure_threshold=3, recovery_timeout=30)
        self.was_injected = True


def test_rate_limiter_can_be_injected():
    """Test that LLMClient accepts an injected RateLimiter instance."""
    # Create mock rate limiter
    mock_limiter = MockRateLimiter()

    # Inject into client
    client = LLMClient(
        endpoint="http://test-endpoint",
        model="test-model",
        rate_limiter=mock_limiter,
        enable_circuit_breaker=False,
    )

    # Verify the injected instance is used
    assert client.rate_limiter is mock_limiter
    assert hasattr(client.rate_limiter, "was_injected")
    assert client.rate_limiter.was_injected is True


def test_circuit_breaker_can_be_injected():
    """Test that LLMClient accepts an injected CircuitBreaker instance."""
    # Create mock circuit breaker
    mock_breaker = MockCircuitBreaker()

    # Inject into client
    client = LLMClient(
        endpoint="http://test-endpoint",
        model="test-model",
        circuit_breaker=mock_breaker,
    )

    # Verify the injected instance is used
    assert client.circuit_breaker is mock_breaker
    assert hasattr(client.circuit_breaker, "was_injected")
    assert client.circuit_breaker.was_injected is True


def test_backward_compatibility_creates_default_rate_limiter():
    """Test that LLMClient creates default RateLimiter if not injected (backward compat)."""
    client = LLMClient(
        endpoint="http://test-endpoint",
        model="test-model",
        enable_circuit_breaker=False,
    )

    # Should have created a RateLimiter automatically
    assert client.rate_limiter is not None
    assert isinstance(client.rate_limiter, RateLimiter)
    # Should not be our mock (no was_injected attribute)
    assert not hasattr(client.rate_limiter, "was_injected")


def test_backward_compatibility_creates_default_circuit_breaker():
    """Test that LLMClient creates default CircuitBreaker if not injected (backward compat)."""
    client = LLMClient(
        endpoint="http://test-endpoint",
        model="test-model",
        enable_circuit_breaker=True,
    )

    # Should have created a CircuitBreaker automatically
    assert client.circuit_breaker is not None
    assert isinstance(client.circuit_breaker, CircuitBreaker)
    # Should not be our mock
    assert not hasattr(client.circuit_breaker, "was_injected")


def test_circuit_breaker_can_be_disabled_with_none():
    """Test that passing circuit_breaker=None disables the circuit breaker."""
    client = LLMClient(
        endpoint="http://test-endpoint",
        model="test-model",
        circuit_breaker=None,
        enable_circuit_breaker=True,  # Should be ignored
    )

    # Circuit breaker should be None
    assert client.circuit_breaker is None


def test_both_dependencies_can_be_injected_together():
    """Test that both RateLimiter and CircuitBreaker can be injected simultaneously."""
    mock_limiter = MockRateLimiter()
    mock_breaker = MockCircuitBreaker()

    client = LLMClient(
        endpoint="http://test-endpoint",
        model="test-model",
        rate_limiter=mock_limiter,
        circuit_breaker=mock_breaker,
    )

    # Verify both are injected
    assert client.rate_limiter is mock_limiter
    assert client.circuit_breaker is mock_breaker
    assert client.rate_limiter.was_injected is True
    assert client.circuit_breaker.was_injected is True


def test_injected_rate_limiter_configuration_respected():
    """Test that injected RateLimiter's configuration is used, not constructor params."""
    # Create a RateLimiter with specific limits
    custom_limiter = RateLimiter(
        max_concurrent=99,  # Custom value
        requests_per_minute=88,  # Custom value
        max_tokens_per_minute=7777,  # Custom value
    )

    # Pass different values to LLMClient constructor
    client = LLMClient(
        endpoint="http://test-endpoint",
        model="test-model",
        rate_limiter=custom_limiter,
        max_concurrent_global=5,  # Should be ignored
        requests_per_minute=60,  # Should be ignored
        max_tokens_per_minute=100000,  # Should be ignored
        enable_circuit_breaker=False,
    )

    # Verify the injected limiter's config is used
    assert client.rate_limiter.max_concurrent == 99
    assert client.rate_limiter.requests_per_minute == 88
    assert client.rate_limiter.max_tokens_per_minute == 7777
