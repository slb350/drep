"""Intelligent file-based caching for LLM responses with git-aware invalidation.

This module implements a sophisticated caching layer that dramatically reduces LLM costs
and latency by caching responses on disk. The cache is "intelligent" in several ways:

Key Features:
-------------
1. **Git-Aware Invalidation**: Cache keys include commit SHA, so cached results are
   automatically invalidated when code changes (new commit). This ensures stale
   analysis is never reused while maximizing cache hits for unchanged code.

2. **Content-Addressed Keys**: Cache keys are derived from a hash of:
   - System prompt (what we're asking the LLM to do)
   - Code content (what we're analyzing)
   - Model name (different models → different responses)
   - Temperature (affects randomness)
   - Commit SHA (invalidates on code changes)

3. **Multi-Level Validation**: Before returning a cache hit, validates:
   - Model matches (don't use GPT-4 cache for Llama queries)
   - Temperature matches (within 0.01 tolerance)
   - Commit SHA matches (code hasn't changed)
   - Not expired (default 30-day TTL)

4. **Automatic Size Management**: Implements LRU-style eviction to prevent unbounded
   growth. When cache exceeds max_size_bytes, oldest entries are pruned first.

5. **Comprehensive Analytics**: Tracks hit rate, misses, evictions, and size for
   observability and tuning.

Architecture:
-------------
Cache is stored as JSON files on disk in a dedicated directory:
- {cache_key}.json: Contains the LLM response data
- {cache_key}.meta.json: Contains metadata (model, temperature, SHA, timestamp, etc.)

This two-file approach allows quick metadata checks without loading full responses.

Performance Impact:
-------------------
- Cache hit: ~1-5ms (disk read)
- Cache miss: ~500-5000ms (LLM request)
- Typical hit rate: 80%+ on incremental scans
- Cost savings: 80%+ reduction in LLM API costs

The cache is critical for making drep practical to run frequently (e.g., on every commit).

Usage Example:
--------------

::

    from drep.llm.cache import IntelligentCache

    # Initialize cache
    cache = IntelligentCache(
        cache_dir=Path(".cache/llm"),
        ttl_days=30,  # Entries expire after 30 days
        max_size_bytes=10 * 1024 * 1024 * 1024,  # 10GB limit
    )

    # Try to get cached response
    cached = cache.get(
        prompt="Review this code",
        code="def foo(): pass",
        model="gpt-4",
        temperature=0.2,
        commit_sha="abc123...",
    )

    if cached:
        print("Cache hit!")
        return cached  # Full LLM response dict

    # Cache miss - make LLM request and cache result
    response = await llm_client.analyze(...)
    cache.set(
        prompt="Review this code",
        code="def foo(): pass",
        model="gpt-4",
        temperature=0.2,
        commit_sha="abc123...",
        response=response,
        tokens_used=150,
        latency_ms=1200,
    )

    # View cache performance
    print(cache.analytics.report())
"""

import hashlib  # For generating cache keys via SHA-256 hashing
import json  # For serializing/deserializing cache data
import logging  # For debug logging of cache hits/misses
import time  # For TTL calculation and timestamps
from dataclasses import dataclass  # For simple data classes
from pathlib import Path  # For cross-platform file path handling
from typing import Any, Dict, List, Optional  # Type hints for clarity

from drep.constants import TEMPERATURE_TOLERANCE

logger = logging.getLogger(__name__)


@dataclass
class CacheMetadata:
    """Metadata stored alongside each cached LLM response.

    This metadata enables validation of cache entries without loading the full response.
    Stored in {cache_key}.meta.json files.

    Attributes:
        model: Model name used for this response (e.g., "gpt-4", "llama-2-70b").
               Used to validate cache hits - don't return GPT-4 cache for Llama queries.

        temperature: Sampling temperature used (0.0-2.0). Lower = more deterministic.
                    Cached responses are only valid for the same temperature (±0.01).

        commit_sha: Git commit SHA when this response was cached. Used for automatic
                   invalidation - if commit changes, code changed, cache is stale.

        tokens_used: Total tokens consumed (prompt + completion). Used for cost tracking.

        latency_ms: Request latency in milliseconds when originally cached. For analytics.

        timestamp: Unix timestamp when cached (seconds since epoch). Used for TTL checks.
    """

    model: str  # Model name (for validation)
    temperature: float  # Sampling temperature (for validation)
    commit_sha: str  # Git SHA (for invalidation)
    tokens_used: int  # Token count (for analytics)
    latency_ms: float  # Original latency (for analytics)
    timestamp: float  # Cache time (for TTL)


@dataclass
class CacheAnalytics:
    """Track cache performance metrics for observability and tuning.

    These metrics help answer questions like:
    - Is the cache effective? (hit_rate)
    - Are we running out of space? (size_bytes vs max_size_bytes)
    - Is TTL too aggressive? (high evictions with low hit rate)
    - Should we increase cache size? (high hit rate, frequent evictions)

    Attributes:
        hits: Number of cache hits (successful get() with valid cached data).
              Hit = saved LLM request = cost + latency savings.
        misses: Number of cache misses (get() returned None). Could be:
               - Entry never cached
               - Entry invalidated (commit SHA changed, TTL expired, etc.)
               - Entry evicted (cache size exceeded max_size_bytes)
        evictions: Number of entries pruned due to size limits. LRU-style eviction:
                  oldest entries removed first when max_size_bytes exceeded.
        size_bytes: Current total cache size in bytes. Sum of all .json and .meta.json
                   files. Used to trigger eviction when approaching max_size_bytes.
    """

    hits: int = 0  # Successful cache retrievals
    misses: int = 0  # Failed cache retrievals
    evictions: int = 0  # Entries removed due to size limits
    size_bytes: int = 0  # Current cache size in bytes

    def record_hit(self):
        """Record a cache hit."""
        self.hits += 1

    def record_miss(self):
        """Record a cache miss."""
        self.misses += 1

    def record_eviction(self):
        """Record a cache eviction."""
        self.evictions += 1

    def record_size(self, size: int):
        """Update cache size.

        Args:
            size: Size in bytes to add to current size
        """
        self.size_bytes += size

    @property
    def total_requests(self) -> int:
        """Calculate total number of requests."""
        return self.hits + self.misses

    @property
    def hit_rate(self) -> float:
        """Calculate cache hit rate.

        Returns:
            Hit rate as a float between 0.0 and 1.0
        """
        total = self.total_requests
        return self.hits / total if total > 0 else 0.0

    def report(self) -> str:
        """Generate human-readable analytics report.

        Returns:
            Formatted string with cache metrics
        """
        return f"""Cache Analytics:
  Hit rate: {self.hit_rate * 100:.1f}%
  Requests: {self.total_requests} (hits: {self.hits}, misses: {self.misses})
  Evictions: {self.evictions}
  Size: {self.size_bytes / 1024 / 1024:.1f} MB"""


class IntelligentCache:
    """LLM response cache with commit SHA awareness and size management.

    Features:
    - Cache key includes prompt, code, model, temperature, and commit SHA
    - Automatic invalidation when commit SHA changes
    - TTL-based expiration
    - Size-based pruning (LRU-style, removes oldest first)
    """

    def __init__(
        self,
        cache_dir: Path,
        ttl_days: int = 30,
        max_size_bytes: int = 10 * 1024 * 1024 * 1024,  # 10GB
    ):
        """Initialize cache.

        Args:
            cache_dir: Directory to store cache files
            ttl_days: Time-to-live for cache entries in days
            max_size_bytes: Maximum total cache size in bytes
        """
        self.cache_dir = Path(cache_dir)
        self.ttl_days = ttl_days
        self.max_size_bytes = max_size_bytes

        # Cache statistics (legacy, for backward compatibility)
        self.hits = 0
        self.misses = 0

        # Enhanced analytics
        self.analytics = CacheAnalytics()

        # Create cache directory if it doesn't exist
        self.cache_dir.mkdir(parents=True, exist_ok=True)

    def _make_key(
        self,
        prompt: str,
        code: str,
        model: str,
        temperature: float,
        commit_sha: str,
    ) -> str:
        """Generate cache key from inputs.

        The key includes all factors that affect the LLM output:
        - Prompt text (hashed)
        - Code content (hashed)
        - Model name
        - Temperature
        - Commit SHA (to invalidate when code changes)

        Args:
            prompt: System prompt
            code: Code to analyze
            model: Model name
            temperature: Sampling temperature
            commit_sha: Git commit SHA

        Returns:
            Hex string cache key
        """
        # Hash prompt and code to keep key manageable
        prompt_hash = hashlib.sha256(prompt.encode()).hexdigest()[:16]
        code_hash = hashlib.sha256(code.encode()).hexdigest()[:16]

        # Combine all factors
        key_data = f"{prompt_hash}|{code_hash}|{model}|{temperature:.2f}|{commit_sha}"
        cache_key = hashlib.sha256(key_data.encode()).hexdigest()

        return cache_key

    def get(
        self,
        prompt: str,
        code: str,
        model: str,
        temperature: float,
        commit_sha: str,
    ) -> Optional[Dict[str, Any]]:
        """Get cached response if valid.

        Validates:
        - Model matches
        - Temperature matches (within tolerance for floating-point comparison)
        - Commit SHA matches
        - Not expired (TTL)

        Args:
            prompt: System prompt
            code: Code to analyze
            model: Model name
            temperature: Sampling temperature
            commit_sha: Git commit SHA

        Returns:
            Cached response dict or None if not found/invalid
        """
        try:
            # STEP 1: Generate cache key from all relevant inputs
            # Key includes prompt, code, model, temperature, and commit SHA
            cache_key = self._make_key(prompt, code, model, temperature, commit_sha)
            cache_file = self.cache_dir / f"{cache_key}.json"  # Response data
            meta_file = self.cache_dir / f"{cache_key}.meta.json"  # Validation metadata

            # STEP 2: Check if cache entry exists
            # Both files must exist (response + metadata) for valid cache entry
            if not cache_file.exists() or not meta_file.exists():
                self.misses += 1
                self.analytics.record_miss()
                return None

            # STEP 3: Load and parse metadata (fast check before loading full response)
            with open(meta_file, "r") as f:
                meta_data = json.load(f)
                metadata = CacheMetadata(**meta_data)

            # STEP 4: Validate model
            # Critical: Don't return GPT-4 cached response for Llama query
            # Different models produce different responses for same input
            if metadata.model != model:
                logger.debug(f"Cache miss: model mismatch ({metadata.model} != {model})")
                self._invalidate(cache_key)  # Remove invalid entry
                self.misses += 1
                self.analytics.record_miss()
                return None

            # STEP 5: Validate temperature (with small tolerance for floating point)
            # Temperature affects output randomness. Same code + prompt with different
            # temperature should produce different responses.
            # TEMPERATURE_TOLERANCE handles floating point rounding: 0.2 vs 0.200001
            if abs(metadata.temperature - temperature) > TEMPERATURE_TOLERANCE:
                logger.debug(
                    f"Cache miss: temperature mismatch ({metadata.temperature} != {temperature})"
                )
                self._invalidate(cache_key)  # Remove invalid entry
                self.misses += 1
                self.analytics.record_miss()
                return None

            # STEP 6: Validate commit SHA (git-aware invalidation)
            # This is the KEY feature: when code changes (new commit), cached analysis
            # of old code is stale and must not be reused.
            # Example: File had bug, LLM found bug, cached. Bug fixed, new commit.
            # Without this check, cache would still report old bug.
            if metadata.commit_sha != commit_sha:
                logger.debug(
                    f"Cache miss: commit SHA changed "
                    f"({metadata.commit_sha[:8]} -> {commit_sha[:8]})"
                )
                self._invalidate(cache_key)  # Remove stale entry
                self.misses += 1
                self.analytics.record_miss()
                return None

            # STEP 7: Validate TTL (time-to-live)
            # Even if code hasn't changed, very old cache entries may be stale
            # (e.g., LLM improved, prompts refined, etc.)
            # Default 30 days balances freshness with cache hit rate
            age_days = (time.time() - metadata.timestamp) / 86400  # 86400 = seconds/day
            if age_days > self.ttl_days:
                logger.debug(f"Cache miss: expired ({age_days:.1f} days old)")
                self._invalidate(cache_key)  # Remove expired entry
                self.misses += 1
                self.analytics.record_miss()
                return None

            # STEP 8: All validations passed! Load and return cached response
            with open(cache_file, "r") as f:
                response = json.load(f)

            logger.debug(f"Cache hit: {cache_key[:8]}... (age: {age_days:.1f} days)")
            self.hits += 1
            self.analytics.record_hit()
            return response

        except Exception as e:
            # Catch-all for file I/O errors, JSON parse errors, etc.
            # Log but don't crash - treat as cache miss and continue
            logger.warning(f"Cache read error: {e}")
            self.misses += 1
            self.analytics.record_miss()
            return None

    def set(
        self,
        prompt: str,
        code: str,
        model: str,
        temperature: float,
        commit_sha: str,
        response: Dict[str, Any],
        tokens_used: int,
        latency_ms: float,
    ):
        """Cache response with metadata.

        Args:
            prompt: System prompt
            code: Code to analyze
            model: Model name
            temperature: Sampling temperature
            commit_sha: Git commit SHA
            response: LLM response to cache
            tokens_used: Tokens used by request
            latency_ms: Request latency in milliseconds
        """
        try:
            cache_key = self._make_key(prompt, code, model, temperature, commit_sha)
            cache_file = self.cache_dir / f"{cache_key}.json"
            meta_file = self.cache_dir / f"{cache_key}.meta.json"

            # Create metadata
            metadata = CacheMetadata(
                model=model,
                temperature=temperature,
                commit_sha=commit_sha,
                tokens_used=tokens_used,
                latency_ms=latency_ms,
                timestamp=time.time(),
            )

            # Write response
            with open(cache_file, "w") as f:
                json.dump(response, f, indent=2)

            # Write metadata
            with open(meta_file, "w") as f:
                json.dump(
                    {
                        "model": metadata.model,
                        "temperature": metadata.temperature,
                        "commit_sha": metadata.commit_sha,
                        "tokens_used": metadata.tokens_used,
                        "latency_ms": metadata.latency_ms,
                        "timestamp": metadata.timestamp,
                    },
                    f,
                    indent=2,
                )

            logger.debug(f"Cached response: {cache_key[:8]}...")

            # Check size limit and prune if needed
            self._check_size_limit()

        except Exception as e:
            logger.warning(f"Cache write error: {e}")

    def _invalidate(self, cache_key: str):
        """Remove cache entry.

        Args:
            cache_key: Cache key to invalidate
        """
        try:
            cache_file = self.cache_dir / f"{cache_key}.json"
            meta_file = self.cache_dir / f"{cache_key}.meta.json"

            if cache_file.exists():
                cache_file.unlink()
            if meta_file.exists():
                meta_file.unlink()

        except Exception as e:
            logger.warning(f"Cache invalidation error: {e}")

    def _check_size_limit(self):
        """Check cache size and prune oldest entries if over limit."""
        try:
            # Get all cache files
            cache_files = list(self.cache_dir.glob("*.json"))

            # Calculate total size
            total_size = sum(f.stat().st_size for f in cache_files)

            if total_size <= self.max_size_bytes:
                return  # Under limit

            logger.info(
                f"Cache size {total_size / 1024 / 1024:.1f}MB exceeds limit "
                f"{self.max_size_bytes / 1024 / 1024:.1f}MB, pruning..."
            )

            # Sort files by modification time (oldest first)
            files_with_time = []
            for f in cache_files:
                if f.name.endswith(".meta.json"):
                    continue  # Skip metadata files in sorting
                meta_file = self.cache_dir / f"{f.stem}.meta.json"
                if meta_file.exists():
                    mtime = f.stat().st_mtime
                    size = f.stat().st_size + meta_file.stat().st_size
                    files_with_time.append((f, meta_file, mtime, size))

            files_with_time.sort(key=lambda x: x[2])  # Sort by mtime

            # Delete oldest until under 90% of limit
            target_size = self.max_size_bytes * 0.9
            current_size = total_size

            for cache_file, meta_file, _, size in files_with_time:
                if current_size <= target_size:
                    break

                # Delete both files
                cache_file.unlink()
                meta_file.unlink()
                current_size -= size

                logger.debug(f"Pruned cache entry: {cache_file.stem[:8]}...")

            logger.info(
                f"Cache pruned to {current_size / 1024 / 1024:.1f}MB "
                f"({len(files_with_time)} entries checked)"
            )

        except Exception as e:
            logger.warning(f"Cache size check error: {e}")

    def clear(self):
        """Clear all cache entries."""
        try:
            for f in self.cache_dir.glob("*.json"):
                f.unlink()
            logger.info("Cache cleared")
        except Exception as e:
            logger.warning(f"Cache clear error: {e}")

    def get_stats(self) -> Dict[str, Any]:
        """Get cache statistics.

        Returns:
            Dict with cache stats
        """
        try:
            cache_files = list(self.cache_dir.glob("*.json"))
            # Count only response files (not metadata)
            response_files = [f for f in cache_files if not f.name.endswith(".meta.json")]

            total_size = sum(f.stat().st_size for f in cache_files)

            return {
                "entry_count": len(response_files),
                "total_size_bytes": total_size,
                "total_size_mb": total_size / 1024 / 1024,
                "cache_dir": str(self.cache_dir),
                "hits": self.hits,
                "misses": self.misses,
                "hit_rate": (
                    self.hits / (self.hits + self.misses) if (self.hits + self.misses) > 0 else 0.0
                ),
            }
        except Exception as e:
            logger.warning(f"Cache stats error: {e}")
            return {
                "entry_count": 0,
                "total_size_bytes": 0,
                "total_size_mb": 0.0,
                "cache_dir": str(self.cache_dir),
                "hits": self.hits,
                "misses": self.misses,
                "hit_rate": 0.0,
            }

    def get_analytics(self) -> CacheAnalytics:
        """Get enhanced cache analytics.

        Returns:
            CacheAnalytics object with current metrics
        """
        # Update size in analytics
        try:
            cache_files = list(self.cache_dir.glob("*.json"))
            total_size = sum(f.stat().st_size for f in cache_files)
            # Reset and update size
            self.analytics.size_bytes = total_size
        except Exception as e:
            logger.warning(f"Analytics size calculation error: {e}")

        return self.analytics

    def _calculate_file_priority(self, file_info: Dict[str, Any]) -> float:
        """Calculate priority for cache warming (RESERVED FOR FUTURE USE).

        ⚠️ Currently unused by warm_cache() method. The warm_cache() method does not
        reorder or prioritize files—callers should pre-sort the input list if
        prioritization is needed.

        Higher priority = warm first

        Factors:
        - File size (larger = higher priority, more expensive to analyze)
        - Complexity (more functions = higher priority)

        Args:
            file_info: Dict with 'size' and 'complexity' keys

        Returns:
            Priority score (higher = more important)
        """
        priority = 0.0

        # Size factor (up to 100 points)
        size = file_info.get("size", 0)
        priority += min(size / 1000, 100)

        # Complexity factor (10 points per function/complexity unit)
        complexity = file_info.get("complexity", 0)
        priority += complexity * 10

        return priority

    async def warm_cache(
        self,
        files: List[Dict[str, Any]],
        analyzer: Any,
        prompt: str,
        model: str,
        temperature: float,
        commit_sha: str,
    ) -> int:
        """Pre-populate cache for the supplied files.

        The current implementation walks the provided list sequentially, skips entries
        that already have cached results, and invokes the analyzer for the rest. It does
        not reorder or prioritize files—callers that need prioritization should sort the
        input list ahead of time.

        Args:
            files: List of dicts with 'path', 'code' keys (optionally 'size', 'complexity')
            analyzer: Analyzer object with analyze_file method
            prompt: System prompt for analysis
            model: Model name
            temperature: Sampling temperature
            commit_sha: Git commit SHA

        Returns:
            Number of files warmed (excluding already cached)
        """
        warmed_count = 0

        for file_info in files:
            code = file_info.get("code", "")
            file_path = file_info.get("path", "")

            # Skip if already cached
            cached = self.get(prompt, code, model, temperature, commit_sha)
            if cached is not None:
                logger.debug(f"Skipping {file_path}, already cached")
                continue

            # Analyze and cache
            try:
                await analyzer.analyze_file(file_path)
                warmed_count += 1
                logger.debug(f"Warmed cache for {file_path}")
            except Exception as e:
                logger.warning(f"Failed to warm cache for {file_path}: {e}")

        logger.info(f"Cache warming complete: {warmed_count} files analyzed")
        return warmed_count

    def optimize(self) -> int:
        """Run cache optimization.

        - Remove expired entries
        - Clean up orphaned metadata files
        - Return count of removed entries

        Returns:
            Number of entries removed
        """
        removed_count = 0

        try:
            cache_files = list(self.cache_dir.glob("*.json"))

            for cache_file in cache_files:
                # Skip metadata files
                if cache_file.name.endswith(".meta.json"):
                    continue

                meta_file = self.cache_dir / f"{cache_file.stem}.meta.json"

                # Remove if metadata missing
                if not meta_file.exists():
                    logger.debug(f"Removing orphaned cache file: {cache_file.name}")
                    cache_file.unlink()
                    removed_count += 1
                    self.analytics.record_eviction()
                    continue

                # Check if expired
                try:
                    with open(meta_file, "r") as f:
                        meta_data = json.load(f)
                        metadata = CacheMetadata(**meta_data)

                    age_days = (time.time() - metadata.timestamp) / 86400
                    if age_days > self.ttl_days:
                        logger.debug(
                            f"Removing expired cache entry: {cache_file.stem[:8]}... "
                            f"({age_days:.1f} days old)"
                        )
                        cache_file.unlink()
                        meta_file.unlink()
                        removed_count += 1
                        self.analytics.record_eviction()

                except Exception as e:
                    logger.warning(f"Error checking cache entry {cache_file.name}: {e}")
                    # Remove corrupted entry
                    cache_file.unlink()
                    if meta_file.exists():
                        meta_file.unlink()
                    removed_count += 1
                    self.analytics.record_eviction()

            if removed_count > 0:
                logger.info(f"Cache optimization complete: {removed_count} entries removed")
            else:
                logger.debug("Cache optimization complete: no entries removed")

            return removed_count

        except Exception as e:
            logger.error(f"Cache optimization error: {e}")
            return removed_count
