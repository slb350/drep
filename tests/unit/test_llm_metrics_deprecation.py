"""Tests for LLM client metrics deprecation warnings.

This test module verifies that legacy metrics properties show deprecation
warnings directing users to the new metrics object.
"""

import warnings

import pytest

from drep.llm.client import LLMClient


@pytest.fixture
def llm_client():
    """Create a test LLM client."""
    return LLMClient(
        endpoint="http://test-endpoint",
        model="test-model",
        enable_circuit_breaker=False,
    )


def test_total_requests_property_shows_deprecation_warning(llm_client):
    """Test that accessing total_requests shows deprecation warning."""
    with warnings.catch_warnings(record=True) as w:
        warnings.simplefilter("always")
        _ = llm_client.total_requests
        assert len(w) == 1
        assert issubclass(w[0].category, DeprecationWarning)
        assert "total_requests is deprecated" in str(w[0].message)
        assert "client.metrics.total_requests" in str(w[0].message)


def test_total_tokens_property_shows_deprecation_warning(llm_client):
    """Test that accessing total_tokens shows deprecation warning."""
    with warnings.catch_warnings(record=True) as w:
        warnings.simplefilter("always")
        _ = llm_client.total_tokens
        assert len(w) == 1
        assert issubclass(w[0].category, DeprecationWarning)
        assert "total_tokens is deprecated" in str(w[0].message)
        assert "client.metrics.total_tokens" in str(w[0].message)


def test_failed_requests_property_shows_deprecation_warning(llm_client):
    """Test that accessing failed_requests shows deprecation warning."""
    with warnings.catch_warnings(record=True) as w:
        warnings.simplefilter("always")
        _ = llm_client.failed_requests
        assert len(w) == 1
        assert issubclass(w[0].category, DeprecationWarning)
        assert "failed_requests is deprecated" in str(w[0].message)
        assert "client.metrics.failed_requests" in str(w[0].message)


def test_legacy_metrics_still_work(llm_client):
    """Test that legacy metrics still return correct values (backward compatibility)."""
    # Set some values directly (simulating usage)
    llm_client._total_requests = 10
    llm_client._total_tokens = 5000
    llm_client._failed_requests = 2

    # Suppress warnings for this test
    with warnings.catch_warnings():
        warnings.simplefilter("ignore")
        assert llm_client.total_requests == 10
        assert llm_client.total_tokens == 5000
        assert llm_client.failed_requests == 2


def test_new_metrics_object_exists(llm_client):
    """Test that new metrics object is available and has expected attributes."""
    assert hasattr(llm_client, "metrics")
    assert hasattr(llm_client.metrics, "total_requests")
    assert hasattr(llm_client.metrics, "total_tokens")
    assert hasattr(llm_client.metrics, "failed_requests")
