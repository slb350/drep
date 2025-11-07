"""Configuration constants for drep.

This module contains application-wide constants to avoid magic numbers
scattered throughout the codebase. Each constant includes documentation
explaining its purpose and impact.
"""

# ===== LLM Client Constants =====

MAX_ESTIMATED_TOKENS: int = 50000
"""Maximum estimated tokens to reserve for a single LLM request.

This caps the token reservation in the rate limiter to prevent over-reservation
that would unnecessarily throttle requests. Set to 50K tokens which is well
below most model context limits but prevents pathological cases.

Used in: drep.llm.client.LLMClient.analyze_code()
Why 50000: Balances safety (prevents over-reservation) with flexibility
           (most requests use < 10K tokens, but large files can use more)
"""


# ===== Cache Constants =====

TEMPERATURE_TOLERANCE: float = 0.01
"""Floating-point tolerance for temperature matching in cache lookups.

When retrieving cached LLM responses, the temperature must match within this
tolerance to account for floating-point rounding errors (0.2 vs 0.200001).
Temperature affects output randomness, so different temperatures should
produce different responses.

Used in: drep.llm.cache.IntelligentCache.get()
Why 0.01: Small enough to detect meaningful differences (0.1 vs 0.2)
          but large enough to handle float rounding (0.2 vs 0.200001)
"""


# ===== Rate Limiter Constants =====

REPO_SEMAPHORE_TTL_SECONDS: int = 600
"""Time-to-live for idle repository semaphores in seconds (10 minutes).

Per-repository semaphores are created lazily to limit concurrent requests
per repo. After this TTL without use, idle semaphores are evicted to prevent
memory leaks when scanning many repositories.

Used in: drep.llm.client.RateLimiter._get_repo_semaphore()
Why 600: 10 minutes provides good balance:
         - Long enough: Won't evict during typical repo scans
         - Short enough: Releases memory for repos scanned hours ago
         - Memory impact: O(active_repos) not O(all_repos_ever)
"""
