"""Tests for circuit breaker pattern."""

import asyncio
from datetime import datetime, timedelta

import pytest

from drep.llm.circuit_breaker import CircuitBreaker, CircuitBreakerOpenError, CircuitState


class TestCircuitBreaker:
    """Tests for CircuitBreaker class."""

    def test_circuit_breaker_initialization(self):
        """Test that CircuitBreaker initializes in CLOSED state."""
        breaker = CircuitBreaker(
            failure_threshold=5,
            recovery_timeout=60,
        )

        assert breaker.state == CircuitState.CLOSED
        assert breaker.failure_count == 0
        assert breaker.last_failure_time is None

    @pytest.mark.asyncio
    async def test_circuit_breaker_allows_calls_when_closed(self):
        """Test that circuit breaker allows calls when CLOSED."""
        breaker = CircuitBreaker(failure_threshold=3)

        async def success_func():
            return "success"

        result = await breaker.call(success_func)
        assert result == "success"
        assert breaker.state == CircuitState.CLOSED

    @pytest.mark.asyncio
    async def test_circuit_breaker_opens_after_threshold(self):
        """Test that circuit breaker opens after failure threshold."""
        breaker = CircuitBreaker(failure_threshold=3)

        async def failing_func():
            raise ValueError("LLM error")

        # Trigger 3 failures
        for _ in range(3):
            with pytest.raises(ValueError):
                await breaker.call(failing_func)

        # Circuit should be OPEN
        assert breaker.state == CircuitState.OPEN
        assert breaker.failure_count == 3

    @pytest.mark.asyncio
    async def test_circuit_breaker_rejects_calls_when_open(self):
        """Test that circuit breaker rejects calls when OPEN."""
        breaker = CircuitBreaker(failure_threshold=2)

        async def failing_func():
            raise ValueError("LLM error")

        # Open the circuit
        for _ in range(2):
            with pytest.raises(ValueError):
                await breaker.call(failing_func)

        assert breaker.state == CircuitState.OPEN

        # Next call should be rejected immediately
        with pytest.raises(CircuitBreakerOpenError):
            await breaker.call(failing_func)

    @pytest.mark.asyncio
    async def test_circuit_breaker_half_open_after_timeout(self):
        """Test that circuit breaker transitions to HALF_OPEN after timeout."""
        breaker = CircuitBreaker(
            failure_threshold=2,
            recovery_timeout=1,  # 1 second
        )

        async def failing_func():
            raise ValueError("LLM error")

        # Open the circuit
        for _ in range(2):
            with pytest.raises(ValueError):
                await breaker.call(failing_func)

        assert breaker.state == CircuitState.OPEN

        # Wait for recovery timeout
        await asyncio.sleep(1.1)

        # Should transition to HALF_OPEN on next call
        async def success_func():
            return "success"

        result = await breaker.call(success_func)

        assert result == "success"
        assert breaker.state == CircuitState.CLOSED  # Should close after success

    @pytest.mark.asyncio
    async def test_circuit_breaker_closes_after_successful_half_open(self):
        """Test that circuit breaker closes after successful calls in HALF_OPEN."""
        breaker = CircuitBreaker(
            failure_threshold=2,
            recovery_timeout=1,
        )

        async def failing_func():
            raise ValueError("LLM error")

        async def success_func():
            return "success"

        # Open the circuit
        for _ in range(2):
            with pytest.raises(ValueError):
                await breaker.call(failing_func)

        # Wait for recovery
        await asyncio.sleep(1.1)

        # First call in HALF_OPEN
        result = await breaker.call(success_func)
        assert result == "success"

        # Should be CLOSED after success
        assert breaker.state == CircuitState.CLOSED

    @pytest.mark.asyncio
    async def test_circuit_breaker_reopens_on_half_open_failure(self):
        """Test that circuit breaker reopens if HALF_OPEN call fails."""
        breaker = CircuitBreaker(
            failure_threshold=2,
            recovery_timeout=1,
        )

        async def failing_func():
            raise ValueError("LLM error")

        # Open the circuit
        for _ in range(2):
            with pytest.raises(ValueError):
                await breaker.call(failing_func)

        # Wait for recovery
        await asyncio.sleep(1.1)

        # Fail in HALF_OPEN
        with pytest.raises(ValueError):
            await breaker.call(failing_func)

        # Should be OPEN again
        assert breaker.state == CircuitState.OPEN

    @pytest.mark.asyncio
    async def test_circuit_breaker_resets_failure_count_on_success(self):
        """Test that successful calls reset failure count."""
        breaker = CircuitBreaker(failure_threshold=5)

        async def failing_func():
            raise ValueError("LLM error")

        async def success_func():
            return "success"

        # 2 failures (below threshold)
        for _ in range(2):
            with pytest.raises(ValueError):
                await breaker.call(failing_func)

        assert breaker.failure_count == 2

        # Success should reset count
        await breaker.call(success_func)

        assert breaker.failure_count == 0
        assert breaker.state == CircuitState.CLOSED

    def test_circuit_breaker_get_state_returns_current_state(self):
        """Test that get_state returns current circuit state."""
        breaker = CircuitBreaker(failure_threshold=3)

        assert breaker.get_state() == CircuitState.CLOSED

    def test_circuit_breaker_get_metrics_returns_stats(self):
        """Test that get_metrics returns circuit breaker statistics."""
        breaker = CircuitBreaker(failure_threshold=3)

        metrics = breaker.get_metrics()

        assert "state" in metrics
        assert "failure_count" in metrics
        assert "success_count" in metrics
        assert metrics["state"] == "closed"


class TestHalfOpenExclusiveProbe:
    """C5: HALF_OPEN is an exclusive single-probe state.

    half_open_max_calls was documented NOT IMPLEMENTED and never enforced;
    concurrent probes are now rejected instead.
    """

    def test_half_open_max_calls_removed(self):
        """The dead config field no longer exists."""
        with pytest.raises(TypeError):
            CircuitBreaker(failure_threshold=2, recovery_timeout=1, half_open_max_calls=3)  # type: ignore[call-arg]
        breaker = CircuitBreaker(failure_threshold=2, recovery_timeout=1)
        assert not hasattr(breaker, "half_open_max_calls")
        assert not hasattr(breaker, "half_open_calls")

    @pytest.mark.asyncio
    async def test_half_open_allows_single_concurrent_probe(self):
        """While a HALF_OPEN probe is in flight, concurrent calls fail fast."""
        breaker = CircuitBreaker(failure_threshold=1, recovery_timeout=1)
        # Force OPEN with an elapsed timeout so the next call transitions to HALF_OPEN
        breaker.state = CircuitState.OPEN
        breaker.last_failure_time = datetime.now() - timedelta(seconds=10)

        started = asyncio.Event()
        release = asyncio.Event()

        async def slow_probe():
            started.set()
            await release.wait()
            return "probe-ok"

        probe = asyncio.create_task(breaker.call(slow_probe))
        await started.wait()

        # Probe is in flight: a concurrent call must be rejected
        with pytest.raises(CircuitBreakerOpenError):
            await breaker.call(self._quick_ok)

        release.set()
        assert await probe == "probe-ok"
        # Probe succeeded: circuit closed, calls flow again
        assert breaker.state == CircuitState.CLOSED
        assert await breaker.call(self._quick_ok) == "ok"

    @staticmethod
    async def _quick_ok():
        return "ok"

    @pytest.mark.asyncio
    async def test_half_open_probe_failure_reopens(self):
        """A failing probe reopens the circuit; the next call waits for timeout."""
        breaker = CircuitBreaker(failure_threshold=1, recovery_timeout=60)
        breaker.state = CircuitState.OPEN
        breaker.last_failure_time = datetime.now() - timedelta(seconds=120)

        async def failing():
            raise RuntimeError("probe failed")

        with pytest.raises(RuntimeError, match="probe failed"):
            await breaker.call(failing)

        assert breaker.state == CircuitState.OPEN
        with pytest.raises(CircuitBreakerOpenError):
            await breaker.call(self._quick_ok)
