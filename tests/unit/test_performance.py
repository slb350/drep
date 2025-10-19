"""Tests for performance optimization tools."""

import asyncio

import pytest

from drep.core.performance import (
    ParallelAnalyzer,
    ProgressTracker,
    timeout_with_partial_results,
)


class TestProgressTracker:
    """Tests for ProgressTracker class."""

    def test_progress_tracker_initialization(self):
        """Test that ProgressTracker initializes correctly."""
        tracker = ProgressTracker(total=100)

        assert tracker.total == 100
        assert tracker.completed == 0
        assert tracker.failed == 0
        assert tracker.skipped == 0

    def test_progress_tracker_updates(self):
        """Test that ProgressTracker updates correctly."""
        tracker = ProgressTracker(total=100)

        tracker.update(completed=10)
        assert tracker.completed == 10

        tracker.update(completed=5, failed=2)
        assert tracker.completed == 15
        assert tracker.failed == 2

        tracker.update(skipped=3)
        assert tracker.skipped == 3

    def test_progress_tracker_calculates_percent(self):
        """Test that ProgressTracker calculates percentage correctly."""
        tracker = ProgressTracker(total=100)

        assert tracker.percent_complete == 0.0

        tracker.update(completed=25)
        assert tracker.percent_complete == 25.0

        tracker.update(completed=25, failed=10)
        # 50 completed + 10 failed = 60 processed out of 100 total = 60%
        assert tracker.percent_complete == 60.0

    def test_progress_tracker_total_processed(self):
        """Test that ProgressTracker calculates total_processed correctly."""
        tracker = ProgressTracker(total=100)

        tracker.update(completed=50, failed=10, skipped=5)

        assert tracker.total_processed == 65
        assert tracker.percent_complete == 65.0

    def test_progress_tracker_generates_report(self):
        """Test that ProgressTracker generates report."""
        tracker = ProgressTracker(total=100)

        tracker.update(completed=50, failed=10, skipped=5)

        report = tracker.report()

        assert "65/100" in report or "65 / 100" in report
        assert "65.0%" in report or "65%" in report
        assert "50 completed" in report or "completed: 50" in report
        assert "10 failed" in report or "failed: 10" in report
        assert "5 skipped" in report or "skipped: 5" in report


class TestParallelAnalyzer:
    """Tests for ParallelAnalyzer class."""

    def test_parallel_analyzer_initialization(self):
        """Test that ParallelAnalyzer initializes correctly."""
        analyzer = ParallelAnalyzer(max_concurrent=5, max_memory_mb=500)

        assert analyzer.max_concurrent == 5
        assert analyzer.max_memory_mb == 500
        assert analyzer.semaphore._value == 5  # Semaphore initial value

    @pytest.mark.asyncio
    async def test_parallel_analyzer_respects_concurrency_limit(self):
        """Test that ParallelAnalyzer respects concurrency limit."""
        analyzer = ParallelAnalyzer(max_concurrent=2)

        # Track concurrent executions
        concurrent_count = 0
        max_concurrent_seen = 0

        async def mock_analyze(item):
            nonlocal concurrent_count, max_concurrent_seen
            concurrent_count += 1
            max_concurrent_seen = max(max_concurrent_seen, concurrent_count)
            await asyncio.sleep(0.1)  # Simulate work
            concurrent_count -= 1
            return f"result_{item}"

        files = ["file1.py", "file2.py", "file3.py", "file4.py"]

        results = await analyzer.analyze_files_parallel(
            files=files,
            analyzer_func=mock_analyze,
        )

        # Should have 4 results
        assert len(results) == 4

        # Should never exceed max_concurrent (2)
        assert max_concurrent_seen <= 2

    @pytest.mark.asyncio
    async def test_parallel_analyzer_handles_failures(self):
        """Test that ParallelAnalyzer handles individual file failures gracefully."""
        analyzer = ParallelAnalyzer(max_concurrent=3)

        async def mock_analyze_with_failures(item):
            if item == "bad_file.py":
                raise ValueError("Bad file!")
            return f"result_{item}"

        files = ["file1.py", "bad_file.py", "file2.py", "file3.py"]

        results = await analyzer.analyze_files_parallel(
            files=files,
            analyzer_func=mock_analyze_with_failures,
        )

        # Should return results for successful files only
        assert len(results) == 3
        assert "result_file1.py" in results
        assert "result_file2.py" in results
        assert "result_file3.py" in results

    @pytest.mark.asyncio
    async def test_parallel_analyzer_with_progress_callback(self):
        """Test that ParallelAnalyzer calls progress callback."""
        analyzer = ParallelAnalyzer(max_concurrent=3)

        progress_updates = []

        def progress_callback(tracker):
            progress_updates.append(tracker.completed)

        async def mock_analyze(item):
            await asyncio.sleep(0.01)
            return f"result_{item}"

        files = ["file1.py", "file2.py", "file3.py"]

        await analyzer.analyze_files_parallel(
            files=files,
            analyzer_func=mock_analyze,
            progress_callback=progress_callback,
        )

        # Should have received progress updates
        assert len(progress_updates) > 0
        # Final update should be 3 completed
        assert progress_updates[-1] == 3

    @pytest.mark.asyncio
    async def test_parallel_analyzer_returns_empty_for_empty_input(self):
        """Test that ParallelAnalyzer handles empty file list."""
        analyzer = ParallelAnalyzer(max_concurrent=3)

        async def mock_analyze(item):
            return f"result_{item}"

        results = await analyzer.analyze_files_parallel(
            files=[],
            analyzer_func=mock_analyze,
        )

        assert results == []


class TestTimeoutWithPartialResults:
    """Tests for timeout_with_partial_results context manager."""

    @pytest.mark.asyncio
    async def test_timeout_returns_partial_results_on_timeout(self):
        """Test that context manager returns partial results on timeout."""
        partial_results = []

        try:
            async with timeout_with_partial_results(0.1, partial_results):
                for i in range(10):
                    partial_results.append(i)
                    await asyncio.sleep(0.05)  # Total would be 0.5s
        except asyncio.TimeoutError:
            pass  # Expected

        # Should have some partial results (not all 10)
        assert len(partial_results) > 0
        assert len(partial_results) < 10

    @pytest.mark.asyncio
    async def test_timeout_completes_normally_if_fast_enough(self):
        """Test that context manager completes normally if work finishes in time."""
        partial_results = []

        # Should NOT timeout
        async with timeout_with_partial_results(1.0, partial_results):
            for i in range(5):
                partial_results.append(i)
                await asyncio.sleep(0.01)  # Total 0.05s

        # Should have all results
        assert len(partial_results) == 5

    @pytest.mark.asyncio
    async def test_timeout_preserves_partial_results(self):
        """Test that partial results are preserved on timeout."""
        partial_results = []

        try:
            async with timeout_with_partial_results(0.2, partial_results):
                # Add some items
                for i in range(100):
                    partial_results.append(i)
                    await asyncio.sleep(0.01)
        except asyncio.TimeoutError:
            pass

        # Partial results should be preserved
        assert len(partial_results) > 0
        # Results should be sequential (no corruption)
        for i, val in enumerate(partial_results):
            assert val == i
