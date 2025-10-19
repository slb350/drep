"""Circuit breaker pattern for LLM resilience.

Prevents cascading failures by tracking error rates and temporarily
blocking requests when failure threshold is exceeded.
"""

import logging
from datetime import datetime, timedelta
from enum import Enum
from typing import Any, Callable, Dict, Optional

logger = logging.getLogger(__name__)


class CircuitState(Enum):
    """Circuit breaker states."""

    CLOSED = "closed"  # Normal operation, requests allowed
    OPEN = "open"  # Failing, reject requests immediately
    HALF_OPEN = "half_open"  # Testing recovery, limited requests


class CircuitBreakerOpenError(Exception):
    """Raised when circuit breaker is open."""

    pass


class CircuitBreaker:
    """Circuit breaker for LLM failures.

    Prevents cascading failures by tracking error rates and temporarily
    blocking requests when failure threshold is exceeded.

    States:
    - CLOSED: Normal operation, all requests allowed
    - OPEN: Too many failures, reject requests immediately
    - HALF_OPEN: Testing if service recovered, allow limited requests

    Transitions:
    - CLOSED → OPEN: After failure_threshold consecutive failures
    - OPEN → HALF_OPEN: After recovery_timeout seconds
    - HALF_OPEN → CLOSED: After half_open_max_calls successes
    - HALF_OPEN → OPEN: On any failure in HALF_OPEN state
    """

    def __init__(
        self,
        failure_threshold: int = 5,
        recovery_timeout: int = 60,
        half_open_max_calls: int = 3,
    ):
        """Initialize circuit breaker.

        Args:
            failure_threshold: Consecutive failures before opening circuit
            recovery_timeout: Seconds to wait before attempting recovery
            half_open_max_calls: Successful calls in HALF_OPEN before closing
        """
        self.failure_threshold = failure_threshold
        self.recovery_timeout = recovery_timeout
        self.half_open_max_calls = half_open_max_calls

        # State tracking
        self.state = CircuitState.CLOSED
        self.failure_count = 0
        self.success_count = 0
        self.last_failure_time: Optional[datetime] = None
        self.half_open_calls = 0

    async def call(self, func: Callable, *args, **kwargs) -> Any:
        """Execute function with circuit breaker protection.

        Args:
            func: Async function to execute
            *args: Positional arguments for func
            **kwargs: Keyword arguments for func

        Returns:
            Result from func

        Raises:
            CircuitBreakerOpenError: If circuit is OPEN
            Exception: Original exception from func if call fails
        """
        # Check if circuit is OPEN
        if self.state == CircuitState.OPEN:
            if self._should_attempt_reset():
                self._transition_to_half_open()
            else:
                raise CircuitBreakerOpenError(
                    f"Circuit breaker is OPEN. "
                    f"Last failure: {self.last_failure_time}, "
                    f"Retry after: {self.recovery_timeout}s"
                )

        # Execute the function
        try:
            result = await func(*args, **kwargs)
            self._on_success()
            return result
        except Exception:
            self._on_failure()
            raise

    def _should_attempt_reset(self) -> bool:
        """Check if enough time has passed to try recovery.

        Returns:
            True if recovery timeout has elapsed
        """
        if self.last_failure_time is None:
            return False

        elapsed = datetime.now() - self.last_failure_time
        return elapsed >= timedelta(seconds=self.recovery_timeout)

    def _transition_to_half_open(self):
        """Move to HALF_OPEN state for testing recovery."""
        logger.info("Circuit breaker transitioning to HALF_OPEN state")
        self.state = CircuitState.HALF_OPEN
        self.half_open_calls = 0

    def _on_success(self):
        """Handle successful call.

        - In CLOSED: Reset failure count
        - In HALF_OPEN: Close circuit immediately on first success
        """
        self.success_count += 1

        if self.state == CircuitState.CLOSED:
            # Reset failure count on success
            self.failure_count = 0

        elif self.state == CircuitState.HALF_OPEN:
            # Any success in HALF_OPEN closes the circuit immediately
            logger.info("Circuit breaker closing after successful recovery")
            self.state = CircuitState.CLOSED
            self.failure_count = 0
            self.half_open_calls = 0

    def _on_failure(self):
        """Handle failed call.

        - In CLOSED: Increment failure count, open if threshold met
        - In HALF_OPEN: Immediately reopen circuit
        """
        self.failure_count += 1
        self.last_failure_time = datetime.now()

        if self.state == CircuitState.CLOSED:
            if self.failure_count >= self.failure_threshold:
                logger.warning(f"Circuit breaker opening after {self.failure_count} failures")
                self.state = CircuitState.OPEN

        elif self.state == CircuitState.HALF_OPEN:
            # Any failure in HALF_OPEN reopens the circuit
            logger.warning("Circuit breaker reopening after HALF_OPEN failure")
            self.state = CircuitState.OPEN
            self.half_open_calls = 0

    def get_state(self) -> CircuitState:
        """Get current circuit state.

        Returns:
            Current CircuitState
        """
        return self.state

    def get_metrics(self) -> Dict[str, Any]:
        """Get circuit breaker metrics.

        Returns:
            Dict with state, counts, and timing info
        """
        return {
            "state": self.state.value,
            "failure_count": self.failure_count,
            "success_count": self.success_count,
            "last_failure_time": (
                self.last_failure_time.isoformat() if self.last_failure_time else None
            ),
        }
