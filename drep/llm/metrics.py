"""LLM usage metrics and cost tracking."""

import json
import logging
from dataclasses import dataclass, field
from datetime import datetime, timedelta
from pathlib import Path
from typing import Any, Dict, List, Optional

logger = logging.getLogger(__name__)


@dataclass
class LLMMetrics:
    """Track LLM usage and performance metrics.

    Tracks:
    - Request counts (total, successful, failed, cached)
    - Token usage (prompt, completion, total)
    - Performance (latency, min/max/avg)
    - Cost estimation (based on token pricing)
    - Per-analyzer breakdown
    """

    # Request metrics
    total_requests: int = 0
    successful_requests: int = 0
    failed_requests: int = 0
    cached_requests: int = 0

    # Token metrics
    total_tokens_prompt: int = 0
    total_tokens_completion: int = 0

    # Performance metrics
    total_latency_ms: float = 0.0
    min_latency_ms: float = float("inf")
    max_latency_ms: float = 0.0

    # Cost estimation (configurable per model)
    cost_per_1k_prompt_tokens: float = 0.0015  # Default for GPT-3.5-turbo
    cost_per_1k_completion_tokens: float = 0.002

    # Session tracking
    session_start: datetime = field(default_factory=datetime.now)
    last_request: Optional[datetime] = None

    # Per-analyzer breakdown
    by_analyzer: Dict[str, Dict[str, int]] = field(default_factory=dict)

    def record_request(
        self,
        analyzer: str,
        success: bool,
        cached: bool,
        tokens_prompt: int,
        tokens_completion: int,
        latency_ms: float,
    ):
        """Record a single LLM request.

        Args:
            analyzer: Name of analyzer (code_quality, docstring, pr_review)
            success: Whether request succeeded
            cached: Whether response was from cache
            tokens_prompt: Prompt tokens used
            tokens_completion: Completion tokens used
            latency_ms: Request latency in milliseconds
        """
        self.total_requests += 1
        self.last_request = datetime.now()

        if success:
            self.successful_requests += 1
        else:
            self.failed_requests += 1

        if cached:
            self.cached_requests += 1

        # Token tracking
        self.total_tokens_prompt += tokens_prompt
        self.total_tokens_completion += tokens_completion

        # Latency tracking
        self.total_latency_ms += latency_ms
        self.min_latency_ms = min(self.min_latency_ms, latency_ms)
        self.max_latency_ms = max(self.max_latency_ms, latency_ms)

        # Per-analyzer tracking
        if analyzer not in self.by_analyzer:
            self.by_analyzer[analyzer] = {
                "requests": 0,
                "tokens_prompt": 0,
                "tokens_completion": 0,
            }

        self.by_analyzer[analyzer]["requests"] += 1
        self.by_analyzer[analyzer]["tokens_prompt"] += tokens_prompt
        self.by_analyzer[analyzer]["tokens_completion"] += tokens_completion

    @property
    def total_tokens(self) -> int:
        """Calculate total tokens used."""
        return self.total_tokens_prompt + self.total_tokens_completion

    @property
    def success_rate(self) -> float:
        """Calculate success rate."""
        if self.total_requests == 0:
            return 0.0
        return self.successful_requests / self.total_requests

    @property
    def cache_hit_rate(self) -> float:
        """Calculate cache hit rate."""
        if self.total_requests == 0:
            return 0.0
        return self.cached_requests / self.total_requests

    @property
    def avg_latency_ms(self) -> float:
        """Calculate average latency."""
        if self.successful_requests == 0:
            return 0.0
        return self.total_latency_ms / self.successful_requests

    @property
    def estimated_cost_usd(self) -> float:
        """Estimate total cost in USD.

        Returns:
            Estimated cost based on token usage and pricing
        """
        prompt_cost = (self.total_tokens_prompt / 1000) * self.cost_per_1k_prompt_tokens
        completion_cost = (self.total_tokens_completion / 1000) * self.cost_per_1k_completion_tokens
        return prompt_cost + completion_cost

    def to_dict(self) -> Dict[str, Any]:
        """Convert to dictionary for JSON serialization.

        Returns:
            Dict representation of metrics
        """
        return {
            "session_start": self.session_start.isoformat(),
            "last_request": (self.last_request.isoformat() if self.last_request else None),
            "total_requests": self.total_requests,
            "successful_requests": self.successful_requests,
            "failed_requests": self.failed_requests,
            "cached_requests": self.cached_requests,
            "success_rate": self.success_rate,
            "cache_hit_rate": self.cache_hit_rate,
            "total_tokens_prompt": self.total_tokens_prompt,
            "total_tokens_completion": self.total_tokens_completion,
            "total_tokens": self.total_tokens,
            "estimated_cost_usd": self.estimated_cost_usd,
            "avg_latency_ms": self.avg_latency_ms,
            "min_latency_ms": (self.min_latency_ms if self.min_latency_ms != float("inf") else 0),
            "max_latency_ms": self.max_latency_ms,
            "by_analyzer": self.by_analyzer,
        }

    def report(self, detailed: bool = False) -> str:
        """Generate human-readable metrics report.

        Args:
            detailed: Include per-analyzer breakdown

        Returns:
            Formatted metrics report
        """
        duration = datetime.now() - self.session_start
        hours = int(duration.total_seconds() // 3600)
        minutes = int((duration.total_seconds() % 3600) // 60)
        seconds = int(duration.total_seconds() % 60)

        report_lines = [
            "===== LLM Usage Report =====",
            f"Session duration: {hours}h {minutes}m {seconds}s",
            f"Total requests: {self.total_requests} "
            f"({self.successful_requests} successful, {self.failed_requests} failed, "
            f"{self.cached_requests} cached)",
            f"Success rate: {self.success_rate * 100:.1f}%",
            f"Cache hit rate: {self.cache_hit_rate * 100:.1f}%",
            "",
            f"Tokens used: {self.total_tokens_prompt:,} prompt + "
            f"{self.total_tokens_completion:,} completion = {self.total_tokens:,} total",
            f"Estimated cost: ${self.estimated_cost_usd:.4f} USD",
            "",
            "Performance:",
            f"  Average latency: {self.avg_latency_ms:.0f}ms",
            f"  Min/Max: "
            f"{self.min_latency_ms if self.min_latency_ms != float('inf') else 0:.0f}ms "
            f"/ {self.max_latency_ms:.0f}ms",
        ]

        if detailed and self.by_analyzer:
            report_lines.append("")
            report_lines.append("By Analyzer:")
            for analyzer, stats in sorted(self.by_analyzer.items()):
                total_tokens = stats["tokens_prompt"] + stats["tokens_completion"]
                report_lines.append(
                    f"  {analyzer}: {stats['requests']} requests ({total_tokens:,} tokens)"
                )

        return "\n".join(report_lines)


class MetricsCollector:
    """Collect and persist metrics across sessions."""

    def __init__(self, metrics_file: Path):
        """Initialize metrics collector.

        Args:
            metrics_file: Path to JSON file for storing metrics
        """
        self.metrics_file = Path(metrics_file)
        self.current_session = LLMMetrics()

        # Create directory if needed
        self.metrics_file.parent.mkdir(parents=True, exist_ok=True)

    async def save(self):
        """Save current session metrics to file."""
        try:
            # Load existing metrics
            history = []
            if self.metrics_file.exists():
                with open(self.metrics_file, "r") as f:
                    history = json.load(f)

            # Append current session
            history.append(self.current_session.to_dict())

            # Write back
            with open(self.metrics_file, "w") as f:
                json.dump(history, f, indent=2)

            logger.info(f"Saved metrics to {self.metrics_file}")

        except Exception as e:
            logger.error(f"Failed to save metrics: {e}")

    def load_history(self, days: int = 30) -> List[Dict[str, Any]]:
        """Load historical metrics.

        Args:
            days: Number of days of history to load

        Returns:
            List of metrics dicts from past sessions
        """
        if not self.metrics_file.exists():
            return []

        try:
            with open(self.metrics_file, "r") as f:
                history = json.load(f)

            # Filter to last N days
            cutoff = datetime.now() - timedelta(days=days)
            filtered = [m for m in history if datetime.fromisoformat(m["session_start"]) > cutoff]

            return filtered

        except Exception as e:
            logger.error(f"Failed to load metrics history: {e}")
            return []

    def aggregate_history(self, days: int = 30) -> LLMMetrics:
        """Aggregate metrics over time period.

        Args:
            days: Number of days to aggregate

        Returns:
            Aggregated LLMMetrics object
        """
        history = self.load_history(days)

        if not history:
            return LLMMetrics()

        # Aggregate all metrics
        aggregated = LLMMetrics()

        for session in history:
            aggregated.total_requests += session.get("total_requests", 0)
            aggregated.successful_requests += session.get("successful_requests", 0)
            aggregated.failed_requests += session.get("failed_requests", 0)
            aggregated.cached_requests += session.get("cached_requests", 0)
            aggregated.total_tokens_prompt += session.get("total_tokens_prompt", 0)
            aggregated.total_tokens_completion += session.get("total_tokens_completion", 0)

            # Update min/max latency
            min_lat = session.get("min_latency_ms", float("inf"))
            max_lat = session.get("max_latency_ms", 0)
            if min_lat != float("inf"):
                aggregated.min_latency_ms = min(aggregated.min_latency_ms, min_lat)
            aggregated.max_latency_ms = max(aggregated.max_latency_ms, max_lat)

        return aggregated
