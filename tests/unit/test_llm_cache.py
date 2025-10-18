"""Unit tests for LLM cache."""

import json
import time

import pytest

from drep.llm.cache import IntelligentCache


@pytest.fixture
def cache_dir(tmp_path):
    """Create temporary cache directory."""
    return tmp_path / "cache"


@pytest.fixture
def cache(cache_dir):
    """Create cache instance."""
    return IntelligentCache(cache_dir, ttl_days=30, max_size_bytes=1024 * 1024)  # 1MB for testing


def test_cache_initialization(cache_dir):
    """Test cache initializes and creates directory."""
    cache = IntelligentCache(cache_dir)

    assert cache.cache_dir == cache_dir
    assert cache_dir.exists()
    assert cache_dir.is_dir()


def test_cache_key_generation(cache):
    """Test cache key includes all relevant factors."""
    key1 = cache._make_key("prompt1", "code1", "model1", 0.2, "abc123")
    key2 = cache._make_key("prompt1", "code1", "model1", 0.2, "abc123")

    # Same inputs should produce same key
    assert key1 == key2

    # Different inputs should produce different keys
    key3 = cache._make_key("prompt2", "code1", "model1", 0.2, "abc123")
    assert key1 != key3

    key4 = cache._make_key("prompt1", "code2", "model1", 0.2, "abc123")
    assert key1 != key4

    key5 = cache._make_key("prompt1", "code1", "model2", 0.2, "abc123")
    assert key1 != key5

    key6 = cache._make_key("prompt1", "code1", "model1", 0.5, "abc123")
    assert key1 != key6

    key7 = cache._make_key("prompt1", "code1", "model1", 0.2, "def456")
    assert key1 != key7


def test_cache_set_and_get(cache):
    """Test setting and getting cached response."""
    response = {"result": "success", "data": [1, 2, 3]}

    cache.set(
        prompt="Test prompt",
        code="def foo(): pass",
        model="test-model",
        temperature=0.2,
        commit_sha="abc123",
        response=response,
        tokens_used=100,
        latency_ms=500,
    )

    # Should hit cache
    cached = cache.get(
        prompt="Test prompt",
        code="def foo(): pass",
        model="test-model",
        temperature=0.2,
        commit_sha="abc123",
    )

    assert cached == response


def test_cache_miss_on_different_inputs(cache):
    """Test cache miss when inputs differ."""
    response = {"result": "success"}

    cache.set(
        prompt="prompt1",
        code="code1",
        model="model1",
        temperature=0.2,
        commit_sha="abc123",
        response=response,
        tokens_used=100,
        latency_ms=500,
    )

    # Different prompt
    assert cache.get("prompt2", "code1", "model1", 0.2, "abc123") is None

    # Different code
    assert cache.get("prompt1", "code2", "model1", 0.2, "abc123") is None

    # Different model
    assert cache.get("prompt1", "code1", "model2", 0.2, "abc123") is None

    # Different temperature
    assert cache.get("prompt1", "code1", "model1", 0.5, "abc123") is None

    # Different commit SHA
    assert cache.get("prompt1", "code1", "model1", 0.2, "def456") is None


def test_cache_invalidates_on_commit_change(cache):
    """Test cache invalidates when commit SHA changes."""
    response = {"result": "success"}

    cache.set(
        prompt="test",
        code="code",
        model="model",
        temperature=0.2,
        commit_sha="abc123",
        response=response,
        tokens_used=100,
        latency_ms=500,
    )

    # Should hit with same commit
    cached = cache.get("test", "code", "model", 0.2, "abc123")
    assert cached == response

    # Should miss with different commit
    cached = cache.get("test", "code", "model", 0.2, "def456")
    assert cached is None


def test_cache_invalidates_on_model_change(cache):
    """Test cache invalidates when model changes."""
    response = {"result": "success"}

    cache.set(
        prompt="test",
        code="code",
        model="model1",
        temperature=0.2,
        commit_sha="abc123",
        response=response,
        tokens_used=100,
        latency_ms=500,
    )

    # Should hit with same model
    cached = cache.get("test", "code", "model1", 0.2, "abc123")
    assert cached == response

    # Should miss with different model
    cached = cache.get("test", "code", "model2", 0.2, "abc123")
    assert cached is None


def test_cache_invalidates_on_temperature_change(cache):
    """Test cache invalidates when temperature changes significantly."""
    response = {"result": "success"}

    cache.set(
        prompt="test",
        code="code",
        model="model",
        temperature=0.2,
        commit_sha="abc123",
        response=response,
        tokens_used=100,
        latency_ms=500,
    )

    # Should hit with same temperature
    cached = cache.get("test", "code", "model", 0.2, "abc123")
    assert cached == response

    # Should hit with negligible difference
    cached = cache.get("test", "code", "model", 0.201, "abc123")
    assert cached == response

    # Should miss with significant difference
    cached = cache.get("test", "code", "model", 0.5, "abc123")
    assert cached is None


def test_cache_ttl_expiration(cache_dir):
    """Test cache invalidates after TTL expires."""
    # Create cache with 0-day TTL for testing
    cache = IntelligentCache(cache_dir, ttl_days=0)

    response = {"result": "success"}

    cache.set(
        prompt="test",
        code="code",
        model="model",
        temperature=0.2,
        commit_sha="abc123",
        response=response,
        tokens_used=100,
        latency_ms=500,
    )

    # Should miss immediately since TTL is 0
    cached = cache.get("test", "code", "model", 0.2, "abc123")
    assert cached is None


def test_cache_creates_metadata_file(cache):
    """Test that cache creates both .json and .meta.json files."""
    response = {"result": "success"}

    cache.set(
        prompt="test",
        code="code",
        model="test-model",
        temperature=0.2,
        commit_sha="abc123",
        response=response,
        tokens_used=100,
        latency_ms=500,
    )

    # Find cache files
    cache_files = list(cache.cache_dir.glob("*.json"))
    assert len(cache_files) == 2  # response.json and meta.json

    # Check metadata content
    meta_files = list(cache.cache_dir.glob("*.meta.json"))
    assert len(meta_files) == 1

    with open(meta_files[0], "r") as f:
        metadata = json.load(f)
        assert metadata["model"] == "test-model"
        assert metadata["temperature"] == 0.2
        assert metadata["commit_sha"] == "abc123"
        assert metadata["tokens_used"] == 100
        assert metadata["latency_ms"] == 500


def test_cache_size_management(cache_dir):
    """Test cache prunes oldest entries when size limit exceeded."""
    # Create cache with tiny size limit
    cache = IntelligentCache(cache_dir, max_size_bytes=500)  # Very small for testing

    # Add multiple entries that will exceed limit
    for i in range(10):
        response = {"result": f"response_{i}", "data": "x" * 100}  # Make it larger
        cache.set(
            prompt=f"prompt_{i}",
            code="code",
            model="model",
            temperature=0.2,
            commit_sha="abc123",
            response=response,
            tokens_used=100,
            latency_ms=500,
        )
        time.sleep(0.01)  # Ensure different mtimes

    # Check that cache was pruned
    stats = cache.get_stats()
    # Should have fewer than 10 entries due to size limit
    assert stats["entry_count"] < 10
    assert stats["total_size_bytes"] < 500 * 0.9  # Should be under target (90% of limit)


def test_cache_clear(cache):
    """Test clearing all cache entries."""
    # Add some entries
    for i in range(3):
        cache.set(
            prompt=f"prompt_{i}",
            code="code",
            model="model",
            temperature=0.2,
            commit_sha="abc123",
            response={"result": i},
            tokens_used=100,
            latency_ms=500,
        )

    stats = cache.get_stats()
    assert stats["entry_count"] == 3

    # Clear cache
    cache.clear()

    stats = cache.get_stats()
    assert stats["entry_count"] == 0


def test_cache_stats(cache):
    """Test cache statistics."""
    stats = cache.get_stats()
    assert stats["entry_count"] == 0
    assert stats["total_size_bytes"] == 0

    # Add entry
    cache.set(
        prompt="test",
        code="code",
        model="model",
        temperature=0.2,
        commit_sha="abc123",
        response={"result": "success"},
        tokens_used=100,
        latency_ms=500,
    )

    stats = cache.get_stats()
    assert stats["entry_count"] == 1
    assert stats["total_size_bytes"] > 0
    assert stats["total_size_mb"] > 0


def test_cache_handles_missing_metadata(cache):
    """Test cache handles missing metadata gracefully."""
    # Manually create cache file without metadata
    cache_key = cache._make_key("test", "code", "model", 0.2, "abc123")
    cache_file = cache.cache_dir / f"{cache_key}.json"

    with open(cache_file, "w") as f:
        json.dump({"result": "success"}, f)

    # Should return None (cache miss) since metadata is missing
    cached = cache.get("test", "code", "model", 0.2, "abc123")
    assert cached is None
