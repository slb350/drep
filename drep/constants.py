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

Performance Impact:
- Prevents token exhaustion that would block all concurrent requests
- Typical requests use <10K tokens, but large files can use 30K+
- Over-reservation wastes rate limiter capacity and throttles unnecessarily
- Too low: Large files may be rejected or truncated
- Too high: Single large request can starve other requests
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

Performance Impact:
- Directly affects cache hit rate and cost savings
- Too strict (e.g., 0.0001): May miss valid cache hits due to float precision
- Too loose (e.g., 0.1): May return cached results for different temperatures
- Current value (0.01): Balances precision with practical float handling
- Cache hit rates >80% are typical with this tolerance
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

MAX_TOKENS_PER_MINUTE: int = 100000
"""Maximum tokens allowed per minute across all requests.

Rate limiting threshold to prevent cost overruns. Typical model limits:
- GPT-4: 10K-40K TPM (depending on tier)
- GPT-3.5: 90K-2M TPM
- Local models: Usually unlimited

Used in: drep.llm.client.LLMClient.__init__(), RateLimiter
Why 100000: Conservative default that works for most use cases:
            - High enough: Won't throttle typical workloads
            - Low enough: Prevents runaway costs with paid APIs
            - Adjustable: Users can override via config

Performance Impact:
- Lower values reduce cost risk but may throttle high-volume scans
- Higher values allow faster processing but increase cost exposure
"""

DEFAULT_MAX_TOKENS_PER_REQUEST: int = 8000
"""Default maximum tokens per LLM request.

Used when max_tokens is not specified. Balances response length with cost.
Most use cases need < 4K tokens, but complex code generation may need more.

Used in: drep.llm.client.LLMClient.analyze_code()
Why 8000: Provides good balance:
         - Sufficient: Handles most code analysis responses
         - Conservative: Prevents excessive token usage
         - Standard: Matches common model defaults

Performance Impact:
- Higher values increase cost but allow longer, more detailed responses
- Lower values reduce cost but may truncate complex analysis
"""
