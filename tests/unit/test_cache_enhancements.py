"""Tests for cache enhancements (analytics, warming, optimization)."""

import json
import time
from unittest.mock import AsyncMock, Mock

import pytest

from drep.llm.cache import CacheAnalytics, IntelligentCache


class TestCacheAnalytics:
    """Tests for CacheAnalytics class."""

    def test_cache_analytics_tracks_hits_and_misses(self):
        """Test that analytics track hits and misses correctly."""
        analytics = CacheAnalytics()

        # Initially zero
        assert analytics.hits == 0
        assert analytics.misses == 0
        assert analytics.evictions == 0

        # Record some hits and misses
        analytics.record_hit()
        analytics.record_hit()
        analytics.record_miss()

        assert analytics.hits == 2
        assert analytics.misses == 1
        assert analytics.total_requests == 3

    def test_cache_analytics_calculates_hit_rate(self):
        """Test that hit rate is calculated correctly."""
        analytics = CacheAnalytics()

        # No requests yet
        assert analytics.hit_rate == 0.0

        # 1 hit, 1 miss = 50%
        analytics.record_hit()
        analytics.record_miss()
        assert analytics.hit_rate == 0.5

        # 3 hits, 1 miss = 75%
        analytics.record_hit()
        analytics.record_hit()
        assert analytics.hit_rate == 0.75

    def test_cache_analytics_tracks_size(self):
        """Test that analytics track cache size."""
        analytics = CacheAnalytics()

        analytics.record_size(1024)  # 1KB
        assert analytics.size_bytes == 1024

        analytics.record_size(2048)  # Add 2KB
        assert analytics.size_bytes == 3072

    def test_cache_analytics_tracks_evictions(self):
        """Test that analytics track evictions."""
        analytics = CacheAnalytics()

        analytics.record_eviction()
        analytics.record_eviction()

        assert analytics.evictions == 2

    def test_cache_analytics_generates_report(self):
        """Test that analytics generate human-readable report."""
        analytics = CacheAnalytics()

        analytics.record_hit()
        analytics.record_hit()
        analytics.record_miss()
        analytics.record_size(1024 * 1024)  # 1MB
        analytics.record_eviction()

        report = analytics.report()

        # Check report contains key metrics
        assert "Hit rate: 66.7%" in report or "Hit rate: 67%" in report
        assert "Evictions: 1" in report
        assert "Size: 1.0 MB" in report


class TestCacheWarming:
    """Tests for cache warming functionality."""

    def test_cache_warming_prioritizes_files(self, tmp_path):
        """Test that cache warming prioritizes files correctly."""
        cache = IntelligentCache(cache_dir=tmp_path)

        # Mock file info with different priorities
        files = [
            {"path": "small.py", "size": 100, "complexity": 2},
            {"path": "large.py", "size": 10000, "complexity": 10},
            {"path": "medium.py", "size": 1000, "complexity": 5},
        ]

        # Calculate priorities
        priorities = [cache._calculate_file_priority(f) for f in files]

        # Large/complex file should have highest priority
        assert priorities[1] > priorities[2] > priorities[0]

    @pytest.mark.asyncio
    async def test_cache_warm_skips_already_cached(self, tmp_path):
        """Test that cache warming skips already cached files."""
        cache = IntelligentCache(cache_dir=tmp_path)

        # Pre-cache one file
        cache.set(
            prompt="test",
            code="def foo(): pass",
            model="test-model",
            temperature=0.2,
            commit_sha="abc123",
            response={"result": "test"},
            tokens_used=100,
            latency_ms=50,
        )

        # Mock analyzer with async method
        analyzer_mock = Mock()
        analyzer_mock.analyze_file = AsyncMock(return_value=[])

        # Warm cache with 2 files (one already cached)
        files = [
            {"path": "test.py", "code": "def foo(): pass"},  # Already cached
            {"path": "new.py", "code": "def bar(): pass"},  # Not cached
        ]

        warmed = await cache.warm_cache(
            files=files,
            analyzer=analyzer_mock,
            prompt="test",
            model="test-model",
            temperature=0.2,
            commit_sha="abc123",
        )

        # Should only warm the new file
        assert warmed == 1
        assert analyzer_mock.analyze_file.call_count == 1


class TestCacheOptimization:
    """Tests for cache optimization functionality."""

    def test_cache_optimize_removes_expired(self, tmp_path):
        """Test that optimization removes expired entries."""
        cache = IntelligentCache(cache_dir=tmp_path, ttl_days=1)

        # Create cache entry with old timestamp
        cache.set(
            prompt="test",
            code="code",
            model="model",
            temperature=0.2,
            commit_sha="abc123",
            response={"result": "test"},
            tokens_used=100,
            latency_ms=50,
        )

        # Manually modify timestamp to be old
        cache_key = cache._make_key("test", "code", "model", 0.2, "abc123")
        meta_file = tmp_path / f"{cache_key}.meta.json"

        with open(meta_file, "r") as f:
            meta = json.load(f)

        # Set timestamp to 2 days ago
        meta["timestamp"] = time.time() - (2 * 86400)

        with open(meta_file, "w") as f:
            json.dump(meta, f)

        # Optimize should remove expired entry
        removed = cache.optimize()
        assert removed == 1

        # Entry should be gone
        result = cache.get("test", "code", "model", 0.2, "abc123")
        assert result is None

    def test_cache_optimize_compacts_storage(self, tmp_path):
        """Test that optimization compacts storage."""
        cache = IntelligentCache(cache_dir=tmp_path)

        # Create multiple cache entries
        for i in range(5):
            cache.set(
                prompt=f"test{i}",
                code=f"code{i}",
                model="model",
                temperature=0.2,
                commit_sha="abc123",
                response={"result": f"test{i}"},
                tokens_used=100,
                latency_ms=50,
            )

        initial_stats = cache.get_stats()
        initial_size = initial_stats["total_size_bytes"]

        # Optimize (may compact or deduplicate)
        cache.optimize()

        # Size should be same or smaller
        final_stats = cache.get_stats()
        final_size = final_stats["total_size_bytes"]

        assert final_size <= initial_size


class TestCacheAnalyticsIntegration:
    """Integration tests for analytics with cache operations."""

    def test_get_analytics_returns_current_state(self, tmp_path):
        """Test that get_analytics returns current analytics state."""
        cache = IntelligentCache(cache_dir=tmp_path)

        # Perform some operations
        cache.set(
            "prompt",
            "code",
            "model",
            0.2,
            "abc123",
            {"result": "test"},
            100,
            50,
        )

        # Hit
        cache.get("prompt", "code", "model", 0.2, "abc123")

        # Miss
        cache.get("other", "code", "model", 0.2, "abc123")

        analytics = cache.get_analytics()

        assert analytics.hits == 1
        assert analytics.misses == 1
        assert analytics.hit_rate == 0.5

    def test_analytics_persists_across_operations(self, tmp_path):
        """Test that analytics accumulate across multiple operations."""
        cache = IntelligentCache(cache_dir=tmp_path)

        # Multiple operations
        for i in range(10):
            cache.set(
                f"prompt{i}",
                f"code{i}",
                "model",
                0.2,
                "abc123",
                {"result": f"test{i}"},
                100,
                50,
            )

        # Mix of hits and misses
        for i in range(10):
            cache.get(f"prompt{i}", f"code{i}", "model", 0.2, "abc123")  # Hit

        for i in range(5):
            cache.get(f"missing{i}", "code", "model", 0.2, "abc123")  # Miss

        analytics = cache.get_analytics()

        assert analytics.hits == 10
        assert analytics.misses == 5
        assert analytics.total_requests == 15
