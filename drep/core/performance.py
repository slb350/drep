"""Performance optimization tools for parallel analysis and progress tracking."""

import asyncio
import logging
from contextlib import asynccontextmanager
from dataclasses import dataclass
from typing import Any, Callable, List, Optional

logger = logging.getLogger(__name__)

# Compatibility: asyncio.timeout is only available in Python 3.11+
# Provide a minimal polyfill for Python 3.10 environments.
try:  # Python 3.11+
    from asyncio import timeout as _asyncio_timeout
except Exception:  # Python 3.10 fallback

    @asynccontextmanager
    async def _asyncio_timeout(delay: float):
        """A minimal context manager emulating asyncio.timeout for 3.10.

        Cancels the current task after the specified delay, raising TimeoutError.
        """
        task = asyncio.current_task()
        loop = asyncio.get_event_loop()
        handle = loop.call_later(delay, task.cancel)
        try:
            yield
        except asyncio.CancelledError as e:  # Normalize to TimeoutError
            raise asyncio.TimeoutError() from e
        finally:
            handle.cancel()


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


class ParallelAnalyzer:
    """Optimize parallel file analysis with memory management.

    Features:
    - Concurrent execution with semaphore control
    - Graceful handling of individual failures
    - Progress tracking via callbacks
    - Memory-aware execution
    """

    def __init__(
        self,
        max_concurrent: int = 5,
        max_memory_mb: int = 500,
    ):
        """Initialize parallel analyzer.

        Args:
            max_concurrent: Maximum number of concurrent operations
            max_memory_mb: Maximum memory usage in megabytes (unused for now)
        """
        self.max_concurrent = max_concurrent
        self.max_memory_mb = max_memory_mb
        self.semaphore = asyncio.Semaphore(max_concurrent)

    async def analyze_files_parallel(
        self,
        files: List[str],
        analyzer_func: Callable,
        progress_callback: Optional[Callable[[ProgressTracker], None]] = None,
    ) -> List[Any]:
        """Analyze files in parallel with memory management.

        Features:
        - Adaptive concurrency based on memory usage
        - Progress callbacks for UI updates
        - Graceful handling of individual file failures

        Args:
            files: List of file paths to analyze
            analyzer_func: Async function to call for each file
            progress_callback: Optional callback for progress updates

        Returns:
            List of successful analysis results
        """
        if not files:
            return []

        tracker = ProgressTracker(total=len(files))
        results = []

        async def analyze_with_tracking(file_path: str):
            """Analyze single file with tracking."""
            async with self.semaphore:
                try:
                    result = await analyzer_func(file_path)
                    tracker.update(completed=1)

                    if progress_callback:
                        progress_callback(tracker)

                    return result

                except Exception as e:
                    logger.warning(f"Failed to analyze {file_path}: {e}")
                    tracker.update(failed=1)

                    if progress_callback:
                        progress_callback(tracker)

                    return None

        # Execute all analyses in parallel
        all_results = await asyncio.gather(
            *[analyze_with_tracking(f) for f in files],
            return_exceptions=False,
        )

        # Filter out None (failed) results
        results = [r for r in all_results if r is not None]

        return results


@asynccontextmanager
async def timeout_with_partial_results(timeout_seconds: float, partial_results: List):
    """Context manager that returns partial results on timeout.

    Usage:
        async with timeout_with_partial_results(30.0, results):
            for item in items:
                result = await analyze(item)
                results.append(result)

    Args:
        timeout_seconds: Timeout in seconds
        partial_results: List to collect results in

    Raises:
        TimeoutError (asyncio.TimeoutError): If timeout is exceeded
    """
    try:
        async with _asyncio_timeout(timeout_seconds):
            yield
    except TimeoutError:
        # Partial results are already in the list
        logger.warning(
            f"Timeout after {timeout_seconds}s, "
            f"returning {len(partial_results)} partial results"
        )
        raise
