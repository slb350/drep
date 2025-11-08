# Migration Guide

This document provides guidance for migrating between versions of drep.

## v0.1.0 → v0.2.0 (Upcoming)

### Deprecated: Legacy Metrics Properties

**Status**: Deprecated in v0.2.0, will be removed in v1.0.0

The legacy metrics properties on `LLMClient` are deprecated in favor of the structured `metrics` object.

#### What's Deprecated

Direct access to these properties on `LLMClient`:
- `client.total_requests`
- `client.total_tokens`
- `client.failed_requests`

#### Migration Path

**Before (v0.1.0)**:
```python
from drep.llm.client import LLMClient

client = LLMClient(endpoint="http://localhost:1234/v1", model="local-model")

# Using legacy properties
print(f"Requests: {client.total_requests}")
print(f"Tokens: {client.total_tokens}")
print(f"Failures: {client.failed_requests}")
```

**After (v0.2.0+)**:
```python
from drep.llm.client import LLMClient

client = LLMClient(endpoint="http://localhost:1234/v1", model="local-model")

# Using metrics object (recommended)
print(f"Requests: {client.metrics.total_requests}")
print(f"Tokens: {client.metrics.total_tokens}")
print(f"Failures: {client.metrics.failed_requests}")
```

#### Backward Compatibility

The legacy properties still work in v0.2.0 but will show a `DeprecationWarning`:

```python
import warnings

# Suppress deprecation warnings if needed (not recommended)
warnings.filterwarnings("ignore", category=DeprecationWarning)

# Or handle them explicitly
with warnings.catch_warnings(record=True) as w:
    warnings.simplefilter("always")
    count = client.total_requests
    if w:
        print(f"Warning: {w[0].message}")
```

#### Additional Metrics Available

The `metrics` object provides additional metrics not available through legacy properties:

```python
# Request breakdown
print(f"Successful: {client.metrics.successful_requests}")
print(f"Cached: {client.metrics.cached_requests}")

# Token breakdown
print(f"Prompt tokens: {client.metrics.total_tokens_prompt}")
print(f"Completion tokens: {client.metrics.total_tokens_completion}")

# Performance metrics
print(f"Avg latency: {client.metrics.avg_latency_ms}ms")
print(f"Success rate: {client.metrics.success_rate * 100}%")
print(f"Cache hit rate: {client.metrics.cache_hit_rate * 100}%")

# Cost estimation
print(f"Estimated cost: ${client.metrics.estimated_cost_usd:.4f}")

# Per-analyzer breakdown
for analyzer, stats in client.metrics.by_analyzer.items():
    print(f"{analyzer}: {stats['requests']} requests, {stats['tokens_prompt']} tokens")
```

### New: Dependency Injection

**Status**: Available in v0.2.0

LLMClient now supports dependency injection for better testability.

#### Advanced Configuration

You can now inject custom `RateLimiter`, `CircuitBreaker`, and `IntelligentCache` instances:

**Custom Rate Limiter**:
```python
from drep.llm.client import LLMClient, RateLimiter

# Create custom rate limiter with specific limits
custom_limiter = RateLimiter(
    max_concurrent=10,           # Allow 10 concurrent requests
    requests_per_minute=100,     # 100 requests per minute
    max_tokens_per_minute=50000, # 50k tokens per minute
    max_concurrent_per_repo=5,   # 5 concurrent per repository
)

client = LLMClient(
    endpoint="http://localhost:1234/v1",
    model="local-model",
    rate_limiter=custom_limiter,
)
```

**Custom Circuit Breaker**:
```python
from drep.llm.circuit_breaker import CircuitBreaker
from drep.llm.client import LLMClient

# Create custom circuit breaker with specific thresholds
custom_breaker = CircuitBreaker(
    failure_threshold=10,  # Open after 10 failures
    recovery_timeout=120,  # Try recovery after 2 minutes
)

client = LLMClient(
    endpoint="http://localhost:1234/v1",
    model="local-model",
    circuit_breaker=custom_breaker,
)

# Or disable circuit breaker entirely
client = LLMClient(
    endpoint="http://localhost:1234/v1",
    model="local-model",
    circuit_breaker=None,  # Explicitly disable
)
```

**Custom Cache**:
```python
from pathlib import Path
from drep.llm.cache import IntelligentCache
from drep.llm.client import LLMClient

# Create custom cache with specific settings
custom_cache = IntelligentCache(
    cache_dir=Path("/tmp/drep-cache"),
    ttl_days=7,                        # Cache for 7 days
    max_size_bytes=5 * 1024**3,        # 5GB cache limit
)

client = LLMClient(
    endpoint="http://localhost:1234/v1",
    model="local-model",
    cache=custom_cache,
)
```

**Shared Dependencies**:
```python
# Share rate limiter and cache across multiple clients
shared_limiter = RateLimiter(max_concurrent=5, ...)
shared_cache = IntelligentCache(cache_dir=Path("/tmp/cache"), ...)

client1 = LLMClient(
    endpoint="http://localhost:1234/v1",
    model="model-1",
    rate_limiter=shared_limiter,
    cache=shared_cache,
)

client2 = LLMClient(
    endpoint="http://localhost:1234/v1",
    model="model-2",
    rate_limiter=shared_limiter,
    cache=shared_cache,
)
```

#### Testing Benefits

Dependency injection makes testing much easier:

```python
from unittest.mock import Mock
from drep.llm.client import LLMClient, RateLimiter, CircuitBreaker

def test_my_feature():
    # Create mock dependencies
    mock_limiter = Mock(spec=RateLimiter)
    mock_breaker = Mock(spec=CircuitBreaker)

    # Inject mocks
    client = LLMClient(
        endpoint="http://test",
        model="test",
        rate_limiter=mock_limiter,
        circuit_breaker=mock_breaker,
    )

    # Now you can verify mock behavior
    # mock_limiter.acquire.assert_called_once()
    # etc.
```

#### Backward Compatibility

All existing code continues to work without changes. If you don't inject dependencies, defaults are created automatically:

```python
# Old code (still works)
client = LLMClient(endpoint="...", model="...")
# Automatically creates default RateLimiter and CircuitBreaker

# New code (with injection)
client = LLMClient(
    endpoint="...",
    model="...",
    rate_limiter=custom_limiter,
)
```

## Need Help?

If you encounter issues during migration:

1. Check the [documentation](docs/)
2. Review the [CHANGELOG](CHANGELOG.md) for breaking changes
3. Open an issue on [GitHub](https://github.com/slb350/drep/issues)
