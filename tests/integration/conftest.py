"""Shared fixtures for integration tests.

The live-LLM fixtures (LM Studio endpoint) are defined once here and reused
by the docstring and code-quality integration tests. The cache is per-test-run
(tmp_path): hermetic and parallel-safe, at the cost of cross-run cache warming.
"""

import os

import pytest

from drep.docstring.generator import DocstringGenerator
from drep.llm.cache import IntelligentCache
from drep.llm.client import LLMClient

DEFAULT_ENDPOINT = "http://localhost:1234/v1"
TEST_MODEL = "qwen/qwen3-30b-a3b-2507"


def llm_test_endpoint() -> str:
    """Endpoint for live-LLM tests, overridable via DREP_TEST_LLM_ENDPOINT."""
    return os.environ.get("DREP_TEST_LLM_ENDPOINT", DEFAULT_ENDPOINT)


@pytest.fixture
async def llm_client(tmp_path):
    """Create LLM client connected to a real OpenAI-compatible endpoint.

    Endpoint is controlled by DREP_TEST_LLM_ENDPOINT (defaults to a local
    LM Studio / Ollama style endpoint).
    """
    cache = IntelligentCache(
        cache_dir=str(tmp_path / "llm_test_cache"),
        ttl_days=7,
        max_size_bytes=1024 * 1024 * 100,  # 100MB
    )

    client = LLMClient(
        endpoint=llm_test_endpoint(),
        model=TEST_MODEL,
        temperature=0.2,
        max_tokens=4000,
        max_concurrent_global=3,
        requests_per_minute=20,
        max_tokens_per_minute=50000,
        cache=cache,
    )

    yield client

    await client.close()


@pytest.fixture
def generator(llm_client):
    """Create DocstringGenerator with the shared LLM client."""
    return DocstringGenerator(llm_client)


@pytest.fixture
def code_analyzer(llm_client):
    """Create CodeQualityAnalyzer with the shared LLM client."""
    from drep.code_quality.analyzer import CodeQualityAnalyzer

    return CodeQualityAnalyzer(llm_client)
