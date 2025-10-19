"""Integration tests for DocstringGenerator with real LLM endpoint.

These tests connect to LM Studio at https://lmstudio.localbrandonfamily.com/v1
and require the Qwen3-30B-A3B model to be running.

Run with: pytest tests/integration/test_docstring_integration.py -v
"""

import pytest

from drep.docstring.generator import DocstringGenerator
from drep.llm.cache import IntelligentCache
from drep.llm.client import LLMClient


@pytest.fixture
async def llm_client():
    """Create LLM client connected to real LM Studio endpoint."""
    # Create cache for faster repeat tests
    cache = IntelligentCache(
        cache_dir="/tmp/drep_test_cache",
        ttl_days=7,
        max_size_bytes=1024 * 1024 * 100,  # 100MB
    )

    client = LLMClient(
        endpoint="https://lmstudio.localbrandonfamily.com/v1",
        model="qwen/qwen3-30b-a3b-2507",
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
    """Create DocstringGenerator with real LLM client."""
    return DocstringGenerator(llm_client)


@pytest.mark.asyncio
@pytest.mark.integration
async def test_generate_docstring_for_missing(generator):
    """Test generating docstring for function missing one."""
    # Python code with function missing docstring
    code = """
def calculate_total(prices: list[float], tax_rate: float) -> float:
    subtotal = sum(prices)
    tax = subtotal * tax_rate
    total = subtotal + tax
    return total
"""

    # Analyze the file
    findings = await generator.analyze_file(
        file_path="billing.py",
        content=code,
        repo_id="test/billing",
        commit_sha="abc123",
    )

    # Should find missing docstring
    assert len(findings) == 1
    finding = findings[0]

    # Verify finding properties
    assert finding.type == "missing-docstring"
    assert finding.severity == "info"
    assert finding.file_path == "billing.py"
    assert "calculate_total" in finding.message

    # Verify suggestion contains Google-style docstring
    assert "```python" in finding.suggestion
    assert "Args:" in finding.suggestion
    assert "Returns:" in finding.suggestion

    # Print for manual inspection
    print("\n=== Generated Docstring ===")
    print(finding.suggestion)


@pytest.mark.asyncio
@pytest.mark.integration
async def test_detect_poor_docstring(generator):
    """Test detecting and improving poor quality docstring."""
    code = '''
def process_data(data: str) -> str:
    """TODO: write this."""
    cleaned = data.strip().lower()
    normalized = cleaned.replace("  ", " ")
    return normalized
'''

    findings = await generator.analyze_file(
        file_path="utils.py",
        content=code,
        repo_id="test/utils",
        commit_sha="def456",
    )

    # Should detect poor docstring
    assert len(findings) == 1
    finding = findings[0]

    # Should be marked as poor-docstring
    assert finding.type == "poor-docstring"
    assert "process_data" in finding.message

    # Should have suggestion for improvement
    assert "```python" in finding.suggestion

    print("\n=== Improved Docstring ===")
    print(finding.suggestion)


@pytest.mark.asyncio
@pytest.mark.integration
async def test_skip_well_documented_function(generator):
    """Test that well-documented functions are not flagged."""
    code = '''
def format_currency(amount: float, currency: str = "USD") -> str:
    """Format a monetary amount as a currency string.

    Formats the given amount with the appropriate currency symbol and
    decimal places based on the specified currency code.

    Args:
        amount: The monetary amount to format.
        currency: ISO 4217 currency code (e.g., "USD", "EUR").

    Returns:
        Formatted currency string (e.g., "$123.45").

    Raises:
        ValueError: If currency code is not recognized.

    Example:
        >>> format_currency(123.45, "USD")
        "$123.45"
    """
    if currency not in CURRENCY_SYMBOLS:
        raise ValueError(f"Unknown currency: {currency}")

    symbol = CURRENCY_SYMBOLS[currency]
    return f"{symbol}{amount:.2f}"
'''

    findings = await generator.analyze_file(
        file_path="formatting.py",
        content=code,
        repo_id="test/formatting",
        commit_sha="ghi789",
    )

    # Should not flag this well-documented function
    assert len(findings) == 0


@pytest.mark.asyncio
@pytest.mark.integration
async def test_cache_works_for_repeat_analysis(generator, llm_client):
    """Test that cache prevents duplicate LLM calls."""
    code = """
def calculate_total(items: list) -> float:
    total = 0.0
    for item in items:
        if item > 0:
            total += item
    return total
"""

    # First analysis - should hit LLM
    initial_stats = llm_client.cache.get_stats()

    await generator.analyze_file(
        file_path="math_utils.py",
        content=code,
        repo_id="test/math",
        commit_sha="same_sha",
    )

    after_first = llm_client.total_requests

    # Second analysis with same commit SHA - should use cache
    await generator.analyze_file(
        file_path="math_utils.py",
        content=code,
        repo_id="test/math",
        commit_sha="same_sha",  # Same SHA
    )

    after_second = llm_client.total_requests
    final_stats = llm_client.cache.get_stats()

    # Verify cache hit
    assert final_stats["hits"] > initial_stats["hits"], "Cache should have hit on second call"

    # Second call should not increase request count (cache hit)
    assert after_second == after_first, "Second analysis should use cache"


@pytest.mark.asyncio
@pytest.mark.integration
async def test_multiple_functions_in_file(generator):
    """Test analyzing file with multiple functions."""
    code = """
def public_func_no_doc(x: int) -> int:
    result = x * 2
    result += 1
    return result

def _private_helper(data):
    return data.strip()

def another_public(name: str, age: int) -> str:
    info = f"{name} is {age} years old"
    return info
"""

    findings = await generator.analyze_file(
        file_path="multi.py",
        content=code,
        repo_id="test/multi",
        commit_sha="jkl012",
    )

    # Should find 2 missing docstrings (only public functions)
    assert len(findings) == 2

    function_names = [f.message for f in findings]
    assert any("public_func_no_doc" in msg for msg in function_names)
    assert any("another_public" in msg for msg in function_names)

    # Should NOT flag private function
    assert not any("_private_helper" in msg for msg in function_names)
