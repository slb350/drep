"""Tests for performance optimization tools."""

from drep.core.performance import (
    ProgressTracker,
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
