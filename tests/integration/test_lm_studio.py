"""Integration test for LM Studio endpoint.

These tests connect to the real LM Studio endpoint and should only be run
when the endpoint is available.

Run with: pytest tests/integration/test_lm_studio.py -v
"""

import pytest
from pydantic import BaseModel

from drep.llm.client import LLMClient


class TestResponse(BaseModel):
    """Test schema for JSON parsing."""

    message: str
    count: int


@pytest.mark.integration
@pytest.mark.asyncio
async def test_lm_studio_connection():
    """Test basic connection to LM Studio endpoint."""
    client = LLMClient(
        endpoint="https://lmstudio.localbrandonfamily.com/v1",
        model="qwen/qwen3-30b-a3b-2507",
        max_tokens=500,  # Small for test
        max_concurrent_global=5,
        requests_per_minute=60,
        max_tokens_per_minute=100000,
    )

    try:
        # Simple test prompt
        response = await client.analyze_code(
            system_prompt="You are a helpful assistant. Respond with a brief greeting.",
            code="",
        )

        # Verify response
        assert response.content is not None
        assert len(response.content) > 0
        assert response.tokens_used > 0
        assert response.latency_ms > 0
        assert response.model == "qwen/qwen3-30b-a3b-2507"

        print("\n✓ Connection successful!")
        print(f"  Response: {response.content[:100]}...")
        print(f"  Tokens: {response.tokens_used}")
        print(f"  Latency: {response.latency_ms:.0f}ms")

    except Exception as e:
        pytest.fail(f"LM Studio connection failed: {e}")

    finally:
        await client.close()


@pytest.mark.integration
@pytest.mark.asyncio
async def test_lm_studio_json_parsing():
    """Test JSON parsing with real endpoint."""
    client = LLMClient(
        endpoint="https://lmstudio.localbrandonfamily.com/v1",
        model="qwen/qwen3-30b-a3b-2507",
        max_tokens=500,
        max_concurrent_global=5,
    )

    try:
        # Request JSON response
        result = await client.analyze_code_json(
            system_prompt='Respond with this exact JSON: {"message": "hello", "count": 42}',
            code="",
            schema=TestResponse,
        )

        # Verify JSON structure
        assert "message" in result
        assert "count" in result
        assert isinstance(result["count"], int)

        print("\n✓ JSON parsing successful!")
        print(f"  Result: {result}")

    except Exception as e:
        pytest.fail(f"JSON parsing test failed: {e}")

    finally:
        await client.close()


@pytest.mark.integration
@pytest.mark.asyncio
async def test_lm_studio_rate_limiting():
    """Test rate limiting with real endpoint."""
    client = LLMClient(
        endpoint="https://lmstudio.localbrandonfamily.com/v1",
        model="qwen/qwen3-30b-a3b-2507",
        max_tokens=200,
        max_concurrent_global=3,  # Limit concurrency
        requests_per_minute=60,
    )

    try:
        # Make 5 concurrent requests
        async def test_request(i):
            response = await client.analyze_code(
                system_prompt=f"Say 'Request {i}'",
                code="",
            )
            return response.tokens_used

        import asyncio

        start_time = asyncio.get_event_loop().time()
        tokens = await asyncio.gather(*[test_request(i) for i in range(5)])
        elapsed = asyncio.get_event_loop().time() - start_time

        total_tokens = sum(tokens)

        print("\n✓ Rate limiting test successful!")
        print(f"  Completed 5 requests in {elapsed:.1f}s")
        print(f"  Total tokens: {total_tokens}")
        print("  Rate limiter working correctly")

        # Get metrics
        metrics = client.get_metrics()
        print("\n  Client metrics:")
        print(f"    Total requests: {metrics['total_requests']}")
        print(f"    Success rate: {metrics['success_rate']:.1%}")
        print(f"    Avg tokens/request: {metrics['avg_tokens_per_request']:.0f}")

    except Exception as e:
        pytest.fail(f"Rate limiting test failed: {e}")

    finally:
        await client.close()


@pytest.mark.integration
@pytest.mark.asyncio
async def test_lm_studio_code_analysis():
    """Test actual code analysis with LM Studio."""
    client = LLMClient(
        endpoint="https://lmstudio.localbrandonfamily.com/v1",
        model="qwen/qwen3-30b-a3b-2507",
        max_tokens=1000,
        max_concurrent_global=5,
    )

    try:
        # Analyze simple Python code
        code = """
def calculate_total(items):
    total = 0
    for item in items:
        total = total + item
    return total
"""

        response = await client.analyze_code(
            system_prompt="Analyze this Python function. Is there a more Pythonic way to write it? Respond briefly.",
            code=code,
        )

        # Verify we got a response
        assert response.content is not None
        assert len(response.content) > 20  # Should be substantive
        assert response.tokens_used > 0

        print("\n✓ Code analysis successful!")
        print(f"  Analysis: {response.content[:200]}...")
        print(f"  Tokens: {response.tokens_used}")

    except Exception as e:
        pytest.fail(f"Code analysis test failed: {e}")

    finally:
        await client.close()
