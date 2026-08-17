"""Progress tracking for long-running analysis.

Once also held ParallelAnalyzer and timeout_with_partial_results, both
deprecated in 1.2.0 with no production callers and removed in 1.3.0.
"""

import logging
from dataclasses import dataclass

logger = logging.getLogger(__name__)


@dataclass
class ProgressTracker:
    """Track progress of long-running operations.

    Attributes:
        total: Total number of items to process
        completed: Number of items completed successfully
        failed: Number of items that failed
        skipped: Number of items skipped
    """

    total: int
    completed: int = 0
    failed: int = 0
    skipped: int = 0

    def update(self, completed: int = 0, failed: int = 0, skipped: int = 0):
        """Update progress counters.

        Args:
            completed: Number of completed items to add
            failed: Number of failed items to add
            skipped: Number of skipped items to add
        """
        self.completed += completed
        self.failed += failed
        self.skipped += skipped

    @property
    def total_processed(self) -> int:
        """Calculate total number of processed items.

        Returns:
            Sum of completed, failed, and skipped items
        """
        return self.completed + self.failed + self.skipped

    @property
    def percent_complete(self) -> float:
        """Calculate completion percentage.

        Returns:
            Percentage of items processed (0-100)
        """
        if self.total == 0:
            return 0.0
        return (self.total_processed / self.total) * 100.0

    def report(self) -> str:
        """Generate progress report.

        Returns:
            Human-readable progress string
        """
        return (
            f"Progress: {self.total_processed}/{self.total} ({self.percent_complete:.1f}%) "
            f"[completed: {self.completed}, failed: {self.failed}, skipped: {self.skipped}]"
        )
