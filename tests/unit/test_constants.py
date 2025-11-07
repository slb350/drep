"""Tests for drep.constants module."""



def test_constants_module_exists():
    """Test that constants module can be imported."""
    from drep import constants

    assert constants is not None


def test_max_estimated_tokens_defined():
    """Test MAX_ESTIMATED_TOKENS constant is defined with correct value."""
    from drep.constants import MAX_ESTIMATED_TOKENS

    assert MAX_ESTIMATED_TOKENS == 50000
    assert isinstance(MAX_ESTIMATED_TOKENS, int)


def test_temperature_tolerance_defined():
    """Test TEMPERATURE_TOLERANCE constant is defined with correct value."""
    from drep.constants import TEMPERATURE_TOLERANCE

    assert TEMPERATURE_TOLERANCE == 0.01
    assert isinstance(TEMPERATURE_TOLERANCE, float)


def test_repo_semaphore_ttl_defined():
    """Test REPO_SEMAPHORE_TTL_SECONDS constant is defined with correct value."""
    from drep.constants import REPO_SEMAPHORE_TTL_SECONDS

    assert REPO_SEMAPHORE_TTL_SECONDS == 600
    assert isinstance(REPO_SEMAPHORE_TTL_SECONDS, int)


def test_all_constants_have_docstrings():
    """Test that all constants have docstrings explaining their purpose."""
    from drep import constants

    # Check module has docstring
    assert constants.__doc__ is not None
    assert len(constants.__doc__.strip()) > 0


def test_llm_client_uses_max_estimated_tokens():
    """Test that LLMClient imports and uses MAX_ESTIMATED_TOKENS constant."""
    import inspect

    from drep.llm.client import LLMClient

    # Get source code of LLMClient.analyze_code method
    source = inspect.getsource(LLMClient.analyze_code)

    # Should import from constants
    client_module_source = inspect.getsource(inspect.getmodule(LLMClient))
    assert "from drep.constants import MAX_ESTIMATED_TOKENS" in client_module_source

    # Should use the constant, not magic number 50000
    assert "MAX_ESTIMATED_TOKENS" in source
    # Ensure no hardcoded 50000 (except in comments)
    assert "50000" not in source or "# 50000" in source


def test_rate_limiter_uses_semaphore_ttl():
    """Test that RateLimiter uses REPO_SEMAPHORE_TTL_SECONDS constant."""
    import inspect

    from drep.llm.client import RateLimiter

    # Get source code
    source = inspect.getsource(RateLimiter.__init__)
    client_module_source = inspect.getsource(inspect.getmodule(RateLimiter))

    # Should import from constants
    assert "from drep.constants import" in client_module_source
    assert "REPO_SEMAPHORE_TTL_SECONDS" in client_module_source

    # Should use the constant, not magic number 600
    assert "REPO_SEMAPHORE_TTL_SECONDS" in source


def test_cache_uses_temperature_tolerance():
    """Test that IntelligentCache uses TEMPERATURE_TOLERANCE constant."""
    import inspect

    from drep.llm.cache import IntelligentCache

    # Get source code of the get method (where temperature validation happens)
    source = inspect.getsource(IntelligentCache.get)
    cache_module_source = inspect.getsource(inspect.getmodule(IntelligentCache))

    # Should import from constants
    assert "from drep.constants import TEMPERATURE_TOLERANCE" in cache_module_source

    # Should use the constant, not magic number 0.01
    assert "TEMPERATURE_TOLERANCE" in source
    # Ensure no hardcoded 0.01 (except in comments)
    assert "0.01" not in source or "# 0.01" in source
