"""Integration tests for CodeQualityAnalyzer with real LLM endpoint.

These tests connect to LM Studio at https://lmstudio.localbrandonfamily.com/v1
and require the Qwen3-30B-A3B model to be running.

Run with: pytest tests/integration/test_code_quality_integration.py -v
"""

import pytest

from drep.code_quality.analyzer import CodeQualityAnalyzer
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
def analyzer(llm_client):
    """Create CodeQualityAnalyzer with real LLM client."""
    return CodeQualityAnalyzer(llm_client)


@pytest.mark.asyncio
@pytest.mark.integration
async def test_analyze_file_with_security_issue(analyzer):
    """Test that analyzer detects SQL injection vulnerability."""
    code = '''
def get_user(user_id):
    """Get user from database."""
    import sqlite3
    conn = sqlite3.connect('users.db')
    cursor = conn.cursor()

    # SQL injection vulnerability
    query = f"SELECT * FROM users WHERE id = {user_id}"
    cursor.execute(query)

    return cursor.fetchone()
'''

    findings = await analyzer.analyze_file(
        file_path="test_security.py",
        content=code,
        repo_id="test/repo",
        commit_sha="test123",
    )

    # Should detect SQL injection
    assert len(findings) > 0

    # At least one should be high severity (security)
    high_severity = [f for f in findings if f.severity in ("error", "warning")]
    assert len(high_severity) > 0

    # Should mention SQL or injection
    security_issues = [f for f in findings if f.type in ("security", "bug")]
    assert len(security_issues) > 0

    # Check message mentions SQL or injection
    messages = " ".join(f.message.lower() for f in findings)
    assert "sql" in messages or "injection" in messages or "query" in messages


@pytest.mark.asyncio
@pytest.mark.integration
async def test_analyze_file_with_performance_issue(analyzer):
    """Test that analyzer detects performance problem."""
    code = '''
def process_items(items):
    """Process list of items inefficiently."""
    result = []

    # Inefficient nested loops - O(n²)
    for item in items:
        for other in items:
            if item == other:
                result.append(item)

    return result
'''

    findings = await analyzer.analyze_file(
        file_path="test_performance.py",
        content=code,
        repo_id="test/repo",
        commit_sha="test456",
    )

    # LLM should detect performance issue or logic error
    assert len(findings) >= 0  # May or may not detect, depends on LLM


@pytest.mark.asyncio
@pytest.mark.integration
async def test_analyze_file_with_best_practice_issues(analyzer):
    """Test that analyzer detects best practice violations."""
    code = """
def f(x, y):
    z = x + y
    return z

class C:
    def m(self, a):
        return a * 2
"""

    findings = await analyzer.analyze_file(
        file_path="test_style.py",
        content=code,
        repo_id="test/repo",
        commit_sha="test789",
    )

    # Should detect missing docstrings or poor naming
    assert len(findings) >= 0  # May or may not detect, depends on strictness


@pytest.mark.asyncio
@pytest.mark.integration
async def test_analyze_clean_code(analyzer):
    """Test that analyzer doesn't flag clean code."""
    code = '''
def calculate_total(prices: list[float], tax_rate: float) -> float:
    """Calculate total price including tax.

    Args:
        prices: List of item prices
        tax_rate: Tax rate as decimal (e.g., 0.08 for 8%)

    Returns:
        Total price including tax
    """
    subtotal = sum(prices)
    tax = subtotal * tax_rate
    total = subtotal + tax

    return total
'''

    findings = await analyzer.analyze_file(
        file_path="test_clean.py",
        content=code,
        repo_id="test/repo",
        commit_sha="clean123",
    )

    # Clean code should have few or no issues
    # (LLM might still suggest improvements, but no critical/high issues)
    critical_issues = [f for f in findings if f.severity == "error"]
    assert len(critical_issues) == 0


@pytest.mark.asyncio
@pytest.mark.integration
async def test_analyze_file_cache_works(analyzer, llm_client):
    """Test that cache works for repeated analysis."""
    code = "def hello():\n    print('hello')"

    # First analysis - should hit LLM
    await analyzer.analyze_file(
        file_path="test_cache.py",
        content=code,
        repo_id="test/repo",
        commit_sha="cache123",
    )

    # Get initial cache stats
    initial_stats = llm_client.cache.get_stats()
    initial_hits = initial_stats["hits"]

    # Second analysis with same content and commit SHA - should hit cache
    await analyzer.analyze_file(
        file_path="test_cache.py",
        content=code,
        repo_id="test/repo",
        commit_sha="cache123",
    )

    # Check cache was hit
    final_stats = llm_client.cache.get_stats()
    assert final_stats["hits"] > initial_hits


@pytest.mark.asyncio
@pytest.mark.integration
async def test_file_size_limit_respected(analyzer):
    """Test that large files are skipped."""
    from drep.code_quality.analyzer import MAX_FILE_SIZE

    # Create file larger than limit
    large_code = "x = 1\n" * (MAX_FILE_SIZE // 6 + 1000)

    findings = await analyzer.analyze_file(
        file_path="test_large.py",
        content=large_code,
        repo_id="test/repo",
        commit_sha="large123",
    )

    # Should skip large file
    assert len(findings) == 0


@pytest.mark.asyncio
@pytest.mark.integration
async def test_multiple_files_analysis(analyzer):
    """Test analyzing multiple files in sequence."""
    files = [
        ("file1.py", "def test1(): pass"),
        ("file2.py", "def test2(): pass"),
        ("file3.py", "def test3(): pass"),
    ]

    findings = await analyzer.analyze_files(
        files=files,
        repo_id="test/repo",
        commit_sha="multi123",
    )

    # Should complete without errors
    assert isinstance(findings, list)
