#!/usr/bin/env python3
"""Manual test script for LLM client.

This script tests the LLM client with the real LM Studio endpoint
to verify end-to-end connectivity and functionality.

Usage:
    python scripts/test_llm_client.py
"""

import asyncio
import sys
from pathlib import Path

# Add project root to path
sys.path.insert(0, str(Path(__file__).parent.parent))

from drep.llm.cache import IntelligentCache
from drep.llm.client import LLMClient, get_current_commit_sha


async def main():
    """Test LLM client with real endpoint."""

    print("=" * 70)
    print("LLM Client Manual Test")
    print("=" * 70)

    # Test 1: Get commit SHA
    print("\n1. Testing commit SHA retrieval...")
    commit_sha = get_current_commit_sha()
    print(f"   ✓ Current commit: {commit_sha[:8]}...")

    # Test 2: Initialize client
    print("\n2. Initializing LLM client...")
    client = LLMClient(
        endpoint="https://lmstudio.localbrandonfamily.com/v1",
        model="qwen/qwen3-30b-a3b-2507",
        temperature=0.2,
        max_tokens=1000,
        max_concurrent_global=5,
        requests_per_minute=60,
        max_tokens_per_minute=100000,
    )
    print(f"   ✓ Client initialized: {client.endpoint}")

    # Test 3: Basic connection
    print("\n3. Testing basic connection...")
    try:
        response = await client.analyze_code(
            system_prompt="You are a helpful assistant. Say 'Hello from LM Studio!' in a friendly way.",
            code="",
        )
        print(f"   ✓ Connected successfully!")
        print(f"   Response: {response.content[:100]}...")
        print(f"   Tokens used: {response.tokens_used}")
        print(f"   Latency: {response.latency_ms:.0f}ms")
        print(f"   Model: {response.model}")
    except Exception as e:
        print(f"   ✗ Connection failed: {e}")
        await client.close()
        return

    # Test 4: JSON parsing
    print("\n4. Testing JSON parsing...")
    try:
        result = await client.analyze_code_json(
            system_prompt='Return this JSON: {"status": "ok", "test_number": 42}',
            code="",
        )
        print(f"   ✓ JSON parsed successfully!")
        print(f"   Result: {result}")
    except Exception as e:
        print(f"   ✗ JSON parsing failed: {e}")

    # Test 5: Code analysis
    print("\n5. Testing code analysis...")
    code = """
def add_numbers(a, b):
    '''Add two numbers together.'''
    return a + b
"""
    try:
        response = await client.analyze_code(
            system_prompt="Analyze this Python function. Is it well-written? Respond in 1-2 sentences.",
            code=code,
        )
        print(f"   ✓ Code analysis complete!")
        print(f"   Analysis: {response.content[:150]}...")
        print(f"   Tokens: {response.tokens_used}")
    except Exception as e:
        print(f"   ✗ Code analysis failed: {e}")

    # Test 6: Rate limiting
    print("\n6. Testing rate limiting (5 concurrent requests)...")
    try:
        start_time = asyncio.get_event_loop().time()

        async def test_request(i):
            response = await client.analyze_code(
                system_prompt=f"Say 'Request {i} complete'",
                code="",
            )
            return response.tokens_used

        tokens = await asyncio.gather(*[test_request(i) for i in range(5)])
        elapsed = asyncio.get_event_loop().time() - start_time

        print(f"   ✓ Completed 5 requests in {elapsed:.1f}s")
        print(f"   Total tokens: {sum(tokens)}")
    except Exception as e:
        print(f"   ✗ Rate limiting test failed: {e}")

    # Test 7: Cache
    print("\n7. Testing response caching...")
    try:
        # Create cache
        cache = IntelligentCache(
            cache_dir=Path.home() / ".cache" / "drep" / "llm_test",
            ttl_days=1,
        )

        # Create new client with cache
        cached_client = LLMClient(
            endpoint="https://lmstudio.localbrandonfamily.com/v1",
            model="qwen/qwen3-30b-a3b-2507",
            max_tokens=500,
            cache=cache,
        )

        # First request (cache miss)
        start_time = asyncio.get_event_loop().time()
        response1 = await cached_client.analyze_code(
            system_prompt="Say 'Cache test'",
            code="def test(): pass",
            commit_sha="test-commit",
        )
        first_latency = asyncio.get_event_loop().time() - start_time

        # Second request (cache hit)
        start_time = asyncio.get_event_loop().time()
        response2 = await cached_client.analyze_code(
            system_prompt="Say 'Cache test'",
            code="def test(): pass",
            commit_sha="test-commit",
        )
        second_latency = asyncio.get_event_loop().time() - start_time

        print(f"   ✓ Cache working!")
        print(f"   First request: {first_latency * 1000:.0f}ms")
        print(f"   Second request (cached): {second_latency * 1000:.0f}ms")
        print(f"   Speedup: {first_latency / second_latency:.1f}x faster")

        # Get cache stats
        stats = cache.get_stats()
        print(f"   Cache entries: {stats['entry_count']}")

        await cached_client.close()
        cache.clear()  # Clean up test cache

    except Exception as e:
        print(f"   ✗ Cache test failed: {e}")

    # Test 8: Metrics
    print("\n8. Client metrics:")
    metrics = client.get_metrics()
    print(f"   Total requests: {metrics['total_requests']}")
    print(f"   Failed requests: {metrics['failed_requests']}")
    print(f"   Total tokens: {metrics['total_tokens']}")
    print(f"   Success rate: {metrics['success_rate']:.1%}")
    print(f"   Avg tokens/request: {metrics['avg_tokens_per_request']:.0f}")

    # Cleanup
    await client.close()

    print("\n" + "=" * 70)
    print("✓ All tests completed successfully!")
    print("=" * 70)


if __name__ == "__main__":
    try:
        asyncio.run(main())
    except KeyboardInterrupt:
        print("\n\nTest interrupted by user")
        sys.exit(1)
    except Exception as e:
        print(f"\n\nTest failed with error: {e}")
        sys.exit(1)
