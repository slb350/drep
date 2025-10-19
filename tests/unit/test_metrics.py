"""Tests for LLM metrics and logging."""

import json
import logging
from datetime import datetime, timedelta

import pytest

from drep.core.logging_config import StructuredFormatter
from drep.llm.metrics import LLMMetrics, MetricsCollector


class TestLLMMetrics:
    """Tests for LLMMetrics class."""

    def test_metrics_initialization(self):
        """Test that LLMMetrics initializes with zero values."""
        metrics = LLMMetrics()

        assert metrics.total_requests == 0
        assert metrics.successful_requests == 0
        assert metrics.failed_requests == 0
        assert metrics.cached_requests == 0
        assert metrics.total_tokens_prompt == 0
        assert metrics.total_tokens_completion == 0
        assert metrics.total_latency_ms == 0.0
        assert metrics.min_latency_ms == float("inf")
        assert metrics.max_latency_ms == 0.0
        assert metrics.by_analyzer == {}

    def test_record_request_updates_counters(self):
        """Test that record_request updates all counters."""
        metrics = LLMMetrics()

        metrics.record_request(
            analyzer="code_quality",
            success=True,
            cached=False,
            tokens_prompt=100,
            tokens_completion=50,
            latency_ms=250.0,
        )

        assert metrics.total_requests == 1
        assert metrics.successful_requests == 1
        assert metrics.failed_requests == 0
        assert metrics.cached_requests == 0
        assert metrics.total_tokens_prompt == 100
        assert metrics.total_tokens_completion == 50
        assert metrics.total_latency_ms == 250.0
        assert metrics.min_latency_ms == 250.0
        assert metrics.max_latency_ms == 250.0

    def test_record_request_tracks_failures(self):
        """Test that failed requests are tracked."""
        metrics = LLMMetrics()

        metrics.record_request(
            analyzer="docstring",
            success=False,
            cached=False,
            tokens_prompt=0,
            tokens_completion=0,
            latency_ms=100.0,
        )

        assert metrics.total_requests == 1
        assert metrics.successful_requests == 0
        assert metrics.failed_requests == 1

    def test_record_request_tracks_cached(self):
        """Test that cached requests are tracked."""
        metrics = LLMMetrics()

        metrics.record_request(
            analyzer="pr_review",
            success=True,
            cached=True,
            tokens_prompt=0,
            tokens_completion=200,
            latency_ms=5.0,
        )

        assert metrics.cached_requests == 1

    def test_record_request_per_analyzer_tracking(self):
        """Test that per-analyzer breakdown is tracked."""
        metrics = LLMMetrics()

        metrics.record_request(
            analyzer="code_quality",
            success=True,
            cached=False,
            tokens_prompt=100,
            tokens_completion=50,
            latency_ms=250.0,
        )

        metrics.record_request(
            analyzer="code_quality",
            success=True,
            cached=False,
            tokens_prompt=150,
            tokens_completion=75,
            latency_ms=300.0,
        )

        metrics.record_request(
            analyzer="docstring",
            success=True,
            cached=False,
            tokens_prompt=80,
            tokens_completion=40,
            latency_ms=200.0,
        )

        assert "code_quality" in metrics.by_analyzer
        assert metrics.by_analyzer["code_quality"]["requests"] == 2
        assert metrics.by_analyzer["code_quality"]["tokens_prompt"] == 250
        assert metrics.by_analyzer["code_quality"]["tokens_completion"] == 125

        assert "docstring" in metrics.by_analyzer
        assert metrics.by_analyzer["docstring"]["requests"] == 1
        assert metrics.by_analyzer["docstring"]["tokens_prompt"] == 80

    def test_total_tokens_property(self):
        """Test that total_tokens calculates correctly."""
        metrics = LLMMetrics()

        metrics.record_request(
            analyzer="code_quality",
            success=True,
            cached=False,
            tokens_prompt=1000,
            tokens_completion=500,
            latency_ms=250.0,
        )

        assert metrics.total_tokens == 1500

    def test_success_rate_calculation(self):
        """Test that success rate is calculated correctly."""
        metrics = LLMMetrics()

        # No requests yet
        assert metrics.success_rate == 0.0

        # 3 successes out of 5 requests
        for i in range(3):
            metrics.record_request(
                analyzer="test",
                success=True,
                cached=False,
                tokens_prompt=100,
                tokens_completion=50,
                latency_ms=200.0,
            )

        for i in range(2):
            metrics.record_request(
                analyzer="test",
                success=False,
                cached=False,
                tokens_prompt=0,
                tokens_completion=0,
                latency_ms=0.0,
            )

        assert metrics.success_rate == 0.6  # 3/5

    def test_cache_hit_rate_calculation(self):
        """Test that cache hit rate is calculated correctly."""
        metrics = LLMMetrics()

        # No requests yet
        assert metrics.cache_hit_rate == 0.0

        # 2 cached out of 5 requests
        for i in range(2):
            metrics.record_request(
                analyzer="test",
                success=True,
                cached=True,
                tokens_prompt=0,
                tokens_completion=100,
                latency_ms=5.0,
            )

        for i in range(3):
            metrics.record_request(
                analyzer="test",
                success=True,
                cached=False,
                tokens_prompt=100,
                tokens_completion=50,
                latency_ms=200.0,
            )

        assert metrics.cache_hit_rate == 0.4  # 2/5

    def test_avg_latency_calculation(self):
        """Test that average latency is calculated correctly."""
        metrics = LLMMetrics()

        # No successful requests yet
        assert metrics.avg_latency_ms == 0.0

        # Add requests with different latencies
        metrics.record_request(
            analyzer="test",
            success=True,
            cached=False,
            tokens_prompt=100,
            tokens_completion=50,
            latency_ms=100.0,
        )

        metrics.record_request(
            analyzer="test",
            success=True,
            cached=False,
            tokens_prompt=100,
            tokens_completion=50,
            latency_ms=200.0,
        )

        metrics.record_request(
            analyzer="test",
            success=True,
            cached=False,
            tokens_prompt=100,
            tokens_completion=50,
            latency_ms=300.0,
        )

        # Average: (100 + 200 + 300) / 3 = 200
        assert metrics.avg_latency_ms == 200.0

    def test_min_max_latency_tracking(self):
        """Test that min and max latency are tracked."""
        metrics = LLMMetrics()

        latencies = [250.0, 100.0, 400.0, 150.0, 300.0]

        for lat in latencies:
            metrics.record_request(
                analyzer="test",
                success=True,
                cached=False,
                tokens_prompt=100,
                tokens_completion=50,
                latency_ms=lat,
            )

        assert metrics.min_latency_ms == 100.0
        assert metrics.max_latency_ms == 400.0

    def test_estimated_cost_calculation(self):
        """Test that cost estimation works correctly."""
        metrics = LLMMetrics()

        # Record request with 1000 prompt tokens and 500 completion tokens
        metrics.record_request(
            analyzer="code_quality",
            success=True,
            cached=False,
            tokens_prompt=1000,
            tokens_completion=500,
            latency_ms=250.0,
        )

        # Cost = (1000 / 1000) * 0.0015 + (500 / 1000) * 0.002
        # Cost = 0.0015 + 0.001 = 0.0025
        expected_cost = 0.0025
        assert abs(metrics.estimated_cost_usd - expected_cost) < 0.0001

    def test_to_dict_serialization(self):
        """Test that to_dict serializes all metrics."""
        metrics = LLMMetrics()

        metrics.record_request(
            analyzer="code_quality",
            success=True,
            cached=False,
            tokens_prompt=100,
            tokens_completion=50,
            latency_ms=250.0,
        )

        result = metrics.to_dict()

        assert "session_start" in result
        assert "last_request" in result
        assert "total_requests" in result
        assert result["total_requests"] == 1
        assert result["successful_requests"] == 1
        assert result["total_tokens_prompt"] == 100
        assert result["total_tokens_completion"] == 50
        assert result["total_tokens"] == 150
        assert "estimated_cost_usd" in result
        assert "success_rate" in result
        assert "cache_hit_rate" in result
        assert "avg_latency_ms" in result
        assert "by_analyzer" in result

    def test_report_formatting(self):
        """Test that report generates readable output."""
        metrics = LLMMetrics()

        metrics.record_request(
            analyzer="code_quality",
            success=True,
            cached=False,
            tokens_prompt=1000,
            tokens_completion=500,
            latency_ms=250.0,
        )

        report = metrics.report(detailed=False)

        assert "Total requests:" in report or "total requests:" in report
        assert "Success rate:" in report or "success rate:" in report
        assert "Cache hit rate:" in report or "cache hit rate:" in report
        assert "Tokens used:" in report or "tokens used:" in report
        assert "Estimated cost:" in report or "estimated cost:" in report

    def test_report_detailed_includes_analyzer_breakdown(self):
        """Test that detailed report includes per-analyzer stats."""
        metrics = LLMMetrics()

        metrics.record_request(
            analyzer="code_quality",
            success=True,
            cached=False,
            tokens_prompt=100,
            tokens_completion=50,
            latency_ms=250.0,
        )

        metrics.record_request(
            analyzer="docstring",
            success=True,
            cached=False,
            tokens_prompt=80,
            tokens_completion=40,
            latency_ms=200.0,
        )

        report = metrics.report(detailed=True)

        assert "By Analyzer:" in report or "by analyzer:" in report
        assert "code_quality" in report
        assert "docstring" in report


class TestMetricsCollector:
    """Tests for MetricsCollector class."""

    def test_metrics_collector_initialization(self, tmp_path):
        """Test that MetricsCollector initializes correctly."""
        metrics_file = tmp_path / "metrics.json"
        collector = MetricsCollector(metrics_file)

        assert collector.metrics_file == metrics_file
        assert collector.current_session is not None
        assert collector.current_session.total_requests == 0

    @pytest.mark.asyncio
    async def test_save_creates_file(self, tmp_path):
        """Test that save creates metrics file."""
        metrics_file = tmp_path / "metrics.json"
        collector = MetricsCollector(metrics_file)

        # Record some activity
        collector.current_session.record_request(
            analyzer="test",
            success=True,
            cached=False,
            tokens_prompt=100,
            tokens_completion=50,
            latency_ms=200.0,
        )

        await collector.save()

        assert metrics_file.exists()

    @pytest.mark.asyncio
    async def test_save_appends_to_history(self, tmp_path):
        """Test that save appends to existing history."""
        metrics_file = tmp_path / "metrics.json"

        # First session
        collector1 = MetricsCollector(metrics_file)
        collector1.current_session.record_request(
            analyzer="test",
            success=True,
            cached=False,
            tokens_prompt=100,
            tokens_completion=50,
            latency_ms=200.0,
        )
        await collector1.save()

        # Second session
        collector2 = MetricsCollector(metrics_file)
        collector2.current_session.record_request(
            analyzer="test",
            success=True,
            cached=False,
            tokens_prompt=150,
            tokens_completion=75,
            latency_ms=250.0,
        )
        await collector2.save()

        # Check history
        with open(metrics_file, "r") as f:
            history = json.load(f)

        assert len(history) == 2
        assert history[0]["total_requests"] == 1
        assert history[1]["total_requests"] == 1

    def test_load_history_returns_recent_sessions(self, tmp_path):
        """Test that load_history filters by date."""
        metrics_file = tmp_path / "metrics.json"

        # Create mock history with different dates
        old_date = (datetime.now() - timedelta(days=60)).isoformat()
        recent_date = (datetime.now() - timedelta(days=5)).isoformat()

        history = [
            {
                "session_start": old_date,
                "total_requests": 10,
                "successful_requests": 10,
                "failed_requests": 0,
                "cached_requests": 0,
                "total_tokens_prompt": 1000,
                "total_tokens_completion": 500,
            },
            {
                "session_start": recent_date,
                "total_requests": 5,
                "successful_requests": 5,
                "failed_requests": 0,
                "cached_requests": 0,
                "total_tokens_prompt": 500,
                "total_tokens_completion": 250,
            },
        ]

        with open(metrics_file, "w") as f:
            json.dump(history, f)

        collector = MetricsCollector(metrics_file)
        recent = collector.load_history(days=30)

        # Should only return the recent one
        assert len(recent) == 1
        assert recent[0]["total_requests"] == 5


class TestStructuredFormatter:
    """Tests for StructuredFormatter class."""

    def test_structured_formatter_outputs_json(self):
        """Test that StructuredFormatter outputs valid JSON."""
        formatter = StructuredFormatter()

        # Create log record
        record = logging.LogRecord(
            name="drep.llm.client",
            level=logging.INFO,
            pathname="client.py",
            lineno=100,
            msg="Test message",
            args=(),
            exc_info=None,
        )

        output = formatter.format(record)

        # Should be valid JSON
        parsed = json.loads(output)
        assert "timestamp" in parsed
        assert "level" in parsed
        assert "logger" in parsed
        assert "message" in parsed
        assert parsed["level"] == "INFO"
        assert parsed["logger"] == "drep.llm.client"
        assert parsed["message"] == "Test message"

    def test_structured_formatter_includes_context_fields(self):
        """Test that context fields are included in output."""
        formatter = StructuredFormatter()

        # Create log record with context
        record = logging.LogRecord(
            name="drep.llm.client",
            level=logging.INFO,
            pathname="client.py",
            lineno=100,
            msg="LLM request",
            args=(),
            exc_info=None,
        )

        # Add context fields
        record.repo_id = "owner/repo"
        record.file_path = "src/main.py"
        record.analyzer = "code_quality"
        record.tokens_used = 150

        output = formatter.format(record)
        parsed = json.loads(output)

        assert parsed["repo_id"] == "owner/repo"
        assert parsed["file_path"] == "src/main.py"
        assert parsed["analyzer"] == "code_quality"
        assert parsed["tokens_used"] == 150

    def test_structured_formatter_includes_exception(self):
        """Test that exceptions are formatted correctly."""
        formatter = StructuredFormatter()

        try:
            raise ValueError("Test error")
        except ValueError:
            import sys

            exc_info = sys.exc_info()

        record = logging.LogRecord(
            name="drep.llm.client",
            level=logging.ERROR,
            pathname="client.py",
            lineno=100,
            msg="Error occurred",
            args=(),
            exc_info=exc_info,
        )

        output = formatter.format(record)
        parsed = json.loads(output)

        assert "exception" in parsed
        assert "ValueError" in parsed["exception"]
        assert "Test error" in parsed["exception"]
