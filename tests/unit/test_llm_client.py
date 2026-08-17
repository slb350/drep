"""Unit tests for LLM client and rate limiter."""

from unittest.mock import AsyncMock, MagicMock

import pytest
from pydantic import BaseModel

from drep.llm.client import LLMClient, LLMResponse

# Test Rate Limiter


@pytest.mark.asyncio
def test_llm_client_initialization():
    """Test LLM client initializes correctly."""
    client = LLMClient(
        endpoint="http://test.local/v1",
        model="test-model",
        api_key="test-key",
        temperature=0.5,
        max_tokens=1000,
    )

    assert client.endpoint == "http://test.local/v1"
    assert client.model == "test-model"
    assert client.temperature == 0.5
    assert client.max_tokens == 1000
    assert client.metrics.total_requests == 0
    assert client.metrics.total_tokens == 0


# Test LLM Client Basic Request


@pytest.mark.asyncio
async def test_llm_client_analyze_code():
    """Test basic analyze_code request (mocked)."""
    client = LLMClient(
        endpoint="http://test.local/v1",
        model="test-model",
        max_concurrent_global=5,
    )

    # Mock OpenAI response
    mock_response = MagicMock()
    mock_response.choices = [MagicMock()]
    mock_response.choices[0].message.content = "This is a test response"
    mock_response.usage.total_tokens = 100
    mock_response.usage.prompt_tokens = 40
    mock_response.usage.completion_tokens = 60
    mock_response.model = "test-model"

    client.client.chat.completions.create = AsyncMock(return_value=mock_response)

    # Make request
    response = await client.analyze_code("Test prompt", "def foo(): pass")

    assert isinstance(response, LLMResponse)
    assert response.content == "This is a test response"
    assert response.tokens_used == 100
    assert response.model == "test-model"
    assert response.latency_ms > 0

    # Check metrics
    assert client.metrics.total_requests == 1
    assert client.metrics.total_tokens == 100


@pytest.mark.asyncio
async def test_llm_client_retry_logic():
    """Test that client retries on failure."""
    client = LLMClient(
        endpoint="http://test.local/v1",
        model="test-model",
        max_retries=3,
        retry_delay=0.01,  # Fast retry for testing
    )

    # Mock: fail twice, succeed third time
    call_count = 0

    async def mock_create(*args, **kwargs):
        nonlocal call_count
        call_count += 1
        if call_count < 3:
            raise Exception("Connection failed")

        # Success on third call
        mock_response = MagicMock()
        mock_response.choices = [MagicMock()]
        mock_response.choices[0].message.content = "Success"
        mock_response.usage.total_tokens = 50
        mock_response.model = "test-model"
        return mock_response

    client.client.chat.completions.create = mock_create

    # Should succeed after retries
    response = await client.analyze_code("Test", "code")
    assert response.content == "Success"
    assert call_count == 3


@pytest.mark.asyncio
async def test_llm_client_retry_exhaustion():
    """Test that client raises exception after all retries fail."""
    client = LLMClient(
        endpoint="http://test.local/v1",
        model="test-model",
        max_retries=2,
        retry_delay=0.01,
    )

    # Mock: always fail
    async def mock_create(*args, **kwargs):
        raise Exception("Connection failed")

    client.client.chat.completions.create = mock_create

    # Should raise after exhausting retries
    with pytest.raises(Exception, match="Connection failed"):
        await client.analyze_code("Test", "code")

    assert client.metrics.failed_requests == 2


# Test JSON Parsing Strategies


@pytest.mark.asyncio
async def test_json_parse_perfect_json():
    """Test parsing perfect JSON."""
    client = LLMClient(endpoint="http://test.local/v1", model="test-model")

    # Mock response with perfect JSON
    mock_response = MagicMock()
    mock_response.choices = [MagicMock()]
    mock_response.choices[0].message.content = '{"result": "success", "count": 42}'
    mock_response.usage.total_tokens = 50
    mock_response.model = "test-model"

    client.client.chat.completions.create = AsyncMock(return_value=mock_response)

    result = await client.analyze_code_json("Return JSON", "")

    assert result == {"result": "success", "count": 42}


@pytest.mark.asyncio
async def test_json_parse_markdown_fence():
    """Test extracting JSON from markdown code fence."""
    client = LLMClient(endpoint="http://test.local/v1", model="test-model")

    # Mock response with markdown fence
    mock_response = MagicMock()
    mock_response.choices = [MagicMock()]
    mock_response.choices[0].message.content = '```json\n{"result": "success"}\n```'
    mock_response.usage.total_tokens = 50
    mock_response.model = "test-model"

    client.client.chat.completions.create = AsyncMock(return_value=mock_response)

    result = await client.analyze_code_json("Return JSON", "")

    assert result == {"result": "success"}


@pytest.mark.asyncio
async def test_json_parse_trailing_comma():
    """Test fixing trailing commas."""
    client = LLMClient(endpoint="http://test.local/v1", model="test-model")

    # Mock response with trailing comma
    mock_response = MagicMock()
    mock_response.choices = [MagicMock()]
    mock_response.choices[0].message.content = '{"result": "success", "items": [1, 2, 3,],}'
    mock_response.usage.total_tokens = 50
    mock_response.model = "test-model"

    client.client.chat.completions.create = AsyncMock(return_value=mock_response)

    result = await client.analyze_code_json("Return JSON", "")

    assert result == {"result": "success", "items": [1, 2, 3]}


@pytest.mark.asyncio
async def test_json_parse_truncated():
    """Test recovering truncated JSON."""
    client = LLMClient(endpoint="http://test.local/v1", model="test-model")

    # Mock response with truncated JSON (missing closing brace)
    mock_response = MagicMock()
    mock_response.choices = [MagicMock()]
    mock_response.choices[0].message.content = '{"result": "success", "count": 42'
    mock_response.usage.total_tokens = 50
    mock_response.model = "test-model"

    client.client.chat.completions.create = AsyncMock(return_value=mock_response)

    result = await client.analyze_code_json("Return JSON", "")

    # Should have recovered by adding closing brace
    assert result == {"result": "success", "count": 42}


@pytest.mark.asyncio
async def test_json_parse_with_pydantic_schema():
    """Test JSON parsing with Pydantic schema validation."""

    class TestSchema(BaseModel):
        result: str
        count: int

    client = LLMClient(endpoint="http://test.local/v1", model="test-model")

    # Mock response
    mock_response = MagicMock()
    mock_response.choices = [MagicMock()]
    mock_response.choices[0].message.content = '{"result": "success", "count": 42}'
    mock_response.usage.total_tokens = 50
    mock_response.model = "test-model"

    client.client.chat.completions.create = AsyncMock(return_value=mock_response)

    result = await client.analyze_code_json("Return JSON", "", schema=TestSchema)

    assert result == {"result": "success", "count": 42}


@pytest.mark.asyncio
async def test_fuzzy_inference():
    """Test fuzzy inference for malformed JSON."""

    class TestSchema(BaseModel):
        result: str
        count: int

    client = LLMClient(endpoint="http://test.local/v1", model="test-model")

    # Create a malformed response that needs 3 attempts to trigger fuzzy inference
    attempt_count = 0

    async def mock_create(*args, **kwargs):
        nonlocal attempt_count
        attempt_count += 1

        mock_response = MagicMock()
        mock_response.choices = [MagicMock()]

        # All attempts return malformed JSON
        # On attempt 3, fuzzy inference should extract values
        mock_response.choices[0].message.content = 'The result is "success" and count is 42'
        mock_response.usage.total_tokens = 50
        mock_response.model = "test-model"

        return mock_response

    client.client.chat.completions.create = mock_create

    result = await client.analyze_code_json("Return JSON", "", schema=TestSchema)

    # Fuzzy inference should extract values
    assert "result" in result
    assert "count" in result
    assert attempt_count == 3  # Should retry to trigger fuzzy inference


# Test Metrics


@pytest.mark.asyncio
async def test_llm_client_metrics():
    """Test client metrics tracking."""
    client = LLMClient(endpoint="http://test.local/v1", model="test-model")

    # Mock successful response
    mock_response = MagicMock()
    mock_response.choices = [MagicMock()]
    mock_response.choices[0].message.content = "Success"
    mock_response.usage.total_tokens = 100
    mock_response.usage.prompt_tokens = 40
    mock_response.usage.completion_tokens = 60
    mock_response.model = "test-model"

    client.client.chat.completions.create = AsyncMock(return_value=mock_response)

    # Make 3 requests
    await client.analyze_code("Test", "code")
    await client.analyze_code("Test", "code")
    await client.analyze_code("Test", "code")

    metrics = client.get_metrics()

    assert metrics["total_requests"] == 3
    assert metrics["failed_requests"] == 0
    assert metrics["total_tokens"] == 300
    assert metrics["success_rate"] == 1.0


@pytest.mark.asyncio
async def test_llm_client_close():
    """Test that client closes properly."""
    client = LLMClient(endpoint="http://test.local/v1", model="test-model")

    # Mock close
    client.client.close = AsyncMock()

    await client.close()

    client.client.close.assert_called_once()


# Test Bedrock Provider Integration


@pytest.mark.asyncio
async def test_llm_client_bedrock_provider_integration():
    """Test LLMClient.analyze_code() with Bedrock provider."""
    import json
    from unittest.mock import patch

    with patch("boto3.client") as mock_boto_client:
        # Mock Bedrock response
        mock_bedrock = MagicMock()
        mock_boto_client.return_value = mock_bedrock

        mock_body = json.dumps(
            {
                "content": [{"type": "text", "text": "Analysis result"}],
                "usage": {"input_tokens": 100, "output_tokens": 50},
            }
        ).encode("utf-8")

        mock_response = {
            "body": MagicMock(read=MagicMock(return_value=mock_body), close=MagicMock())
        }
        mock_bedrock.invoke_model = MagicMock(return_value=mock_response)

        client = LLMClient(
            endpoint="http://ignored",
            model="ignored",
            provider="bedrock",
            bedrock_region="us-east-1",
            bedrock_model="anthropic.claude-sonnet-4-5-20250929-v1:0",
        )

        response = await client.analyze_code(
            system_prompt="Test prompt",
            code="def foo(): pass",
        )

        assert response.content == "Analysis result"
        assert response.tokens_used == 150
        assert mock_bedrock.invoke_model.called


def test_llm_client_bedrock_provider_missing_config():
    """Test LLMClient raises ValueError when Bedrock provider lacks config."""
    with pytest.raises(ValueError, match="Bedrock provider requires bedrock_region"):
        LLMClient(
            endpoint="http://localhost:11434/v1",
            model="test",
            provider="bedrock",
            # Missing bedrock_region and bedrock_model
        )


@pytest.mark.asyncio
async def test_llm_client_bedrock_analyze_code_json():
    """Test LLMClient.analyze_code_json() with Bedrock provider (Gap #1 from PR review)."""
    import json
    from unittest.mock import patch

    with patch("boto3.client") as mock_boto_client:
        # Mock Bedrock response with JSON
        mock_bedrock = MagicMock()
        mock_boto_client.return_value = mock_bedrock

        mock_body = json.dumps(
            {
                "content": [{"type": "text", "text": '{"issues": ["bug1", "bug2"], "count": 2}'}],
                "usage": {"input_tokens": 100, "output_tokens": 50},
            }
        ).encode("utf-8")

        mock_response = {
            "body": MagicMock(read=MagicMock(return_value=mock_body), close=MagicMock())
        }
        mock_bedrock.invoke_model = MagicMock(return_value=mock_response)

        client = LLMClient(
            endpoint="http://ignored",
            model="ignored",
            provider="bedrock",
            bedrock_region="us-east-1",
            bedrock_model="anthropic.claude-sonnet-4-5-20250929-v1:0",
        )

        result = await client.analyze_code_json(
            system_prompt="Find issues",
            code="def foo(): pass",
        )

        # Verify JSON parsed correctly
        assert "issues" in result
        assert result["count"] == 2
        assert len(result["issues"]) == 2


@pytest.mark.asyncio
async def test_llm_client_bedrock_retry_on_throttling():
    """Test LLMClient retries on Bedrock ThrottlingException (Gap #2 from PR review)."""
    import json
    from unittest.mock import patch

    from botocore.exceptions import ClientError

    with patch("boto3.client") as mock_boto_client:
        mock_bedrock = MagicMock()
        mock_boto_client.return_value = mock_bedrock

        # First call: ThrottlingException
        # Second call: Success
        call_count = 0

        def invoke_with_throttle(*args, **kwargs):
            nonlocal call_count
            call_count += 1

            if call_count == 1:
                # First call fails with throttling
                error_response = {
                    "Error": {"Code": "ThrottlingException", "Message": "Rate exceeded"}
                }
                raise ClientError(error_response, "invoke_model")
            # Second call succeeds
            mock_body = json.dumps(
                {
                    "content": [{"type": "text", "text": "Success"}],
                    "usage": {"input_tokens": 10, "output_tokens": 5},
                }
            ).encode("utf-8")
            return {"body": MagicMock(read=MagicMock(return_value=mock_body), close=MagicMock())}

        mock_bedrock.invoke_model = MagicMock(side_effect=invoke_with_throttle)

        client = LLMClient(
            endpoint="http://ignored",
            model="ignored",
            provider="bedrock",
            bedrock_region="us-east-1",
            bedrock_model="anthropic.claude-sonnet-4-5-20250929-v1:0",
            max_retries=3,
            retry_delay=0.01,  # Fast retry for testing
        )

        response = await client.analyze_code(system_prompt="Test", code="def foo(): pass")

        # Should succeed after retry
        assert response.content == "Success"
        assert call_count == 2  # Verify it retried once


@pytest.mark.asyncio
async def test_llm_client_bedrock_cache_integration():
    """Test LLMClient cache hit/miss with Bedrock provider (Gap #3 from PR review)."""
    import json
    import tempfile
    from pathlib import Path
    from unittest.mock import patch

    from drep.llm.cache import IntelligentCache

    with tempfile.TemporaryDirectory() as temp_dir, patch("boto3.client") as mock_boto_client:
        mock_bedrock = MagicMock()
        mock_boto_client.return_value = mock_bedrock

        mock_body = json.dumps(
            {
                "content": [{"type": "text", "text": "Cached response"}],
                "usage": {"input_tokens": 100, "output_tokens": 50},
            }
        ).encode("utf-8")

        mock_response = {
            "body": MagicMock(read=MagicMock(return_value=mock_body), close=MagicMock())
        }
        mock_bedrock.invoke_model = MagicMock(return_value=mock_response)

        # Create cache instance
        cache = IntelligentCache(cache_dir=Path(temp_dir), ttl_days=30)

        client = LLMClient(
            endpoint="http://ignored",
            model="ignored",
            provider="bedrock",
            bedrock_region="us-east-1",
            bedrock_model="anthropic.claude-sonnet-4-5-20250929-v1:0",
            cache=cache,
        )

        # First call - cache miss
        response1 = await client.analyze_code(system_prompt="Test", code="def foo(): pass")
        assert response1.content == "Cached response"
        assert mock_bedrock.invoke_model.call_count == 1

        # Second call - cache hit (same prompt + code)
        response2 = await client.analyze_code(system_prompt="Test", code="def foo(): pass")
        assert response2.content == "Cached response"
        # Should NOT call Bedrock again
        assert mock_bedrock.invoke_model.call_count == 1  # Still 1


@pytest.mark.asyncio
async def test_llm_client_bedrock_with_code_quality_analyzer():
    """Test Bedrock with CodeQualityAnalyzer end-to-end (Gap #4 from PR review)."""
    import json
    from unittest.mock import patch

    from drep.code_quality.analyzer import CodeQualityAnalyzer

    with patch("boto3.client") as mock_boto_client:
        mock_bedrock = MagicMock()
        mock_boto_client.return_value = mock_bedrock

        # Mock Bedrock response with code quality findings
        # Must match CodeAnalysisResult schema (issues, not findings)
        mock_body = json.dumps(
            {
                "content": [
                    {
                        "type": "text",
                        "text": json.dumps(
                            {
                                "summary": "Found 1 issue",
                                "issues": [  # Must be "issues" not "findings"
                                    {
                                        "line": 1,  # Required
                                        "severity": "high",  # Required
                                        "category": "bug",  # Required
                                        "message": "Potential bug found",  # Required
                                        "suggestion": "Fix the bug",  # Required
                                        "code_snippet": "def foo():",  # Required
                                    }
                                ],
                            }
                        ),
                    }
                ],
                "usage": {"input_tokens": 200, "output_tokens": 100},
            }
        ).encode("utf-8")

        mock_response = {
            "body": MagicMock(read=MagicMock(return_value=mock_body), close=MagicMock())
        }
        mock_bedrock.invoke_model = MagicMock(return_value=mock_response)

        client = LLMClient(
            endpoint="http://ignored",
            model="ignored",
            provider="bedrock",
            bedrock_region="us-east-1",
            bedrock_model="anthropic.claude-sonnet-4-5-20250929-v1:0",
        )

        analyzer = CodeQualityAnalyzer(client)

        # Analyze Python code - returns list of Finding objects
        findings_list = await analyzer.analyze_file(
            file_path="test.py",
            content="def foo():\n    pass",
            repo_id="test/repo",
            commit_sha="abc123",
        )

        # Verify analyzer works with Bedrock
        assert isinstance(findings_list, list)
        assert len(findings_list) > 0
        # Note: CodeAnalysisResult.to_findings() converts "high" → "error"
        assert findings_list[0].severity == "error"  # "high" is converted to "error"
        assert findings_list[0].type == "bug"  # Field is "type" not "category"
        assert findings_list[0].message == "Potential bug found"
        assert mock_bedrock.invoke_model.called


@pytest.mark.asyncio
async def test_llm_client_bedrock_preserves_model_name():
    """Test LLMClient preserves Bedrock model name in self.model (P1 cache bug)."""
    import json
    from pathlib import Path
    from tempfile import TemporaryDirectory
    from unittest.mock import MagicMock, patch

    from drep.llm.cache import IntelligentCache
    from drep.llm.client import LLMClient

    with patch("boto3.client") as mock_boto_client:
        mock_bedrock = MagicMock()
        mock_boto_client.return_value = mock_bedrock

        # Mock Bedrock response
        mock_body = json.dumps(
            {
                "content": [{"type": "text", "text": "Test response"}],
                "usage": {"input_tokens": 10, "output_tokens": 5},
            }
        ).encode("utf-8")
        mock_response = {
            "body": MagicMock(read=MagicMock(return_value=mock_body), close=MagicMock())
        }

        async def mock_invoke(*args, **kwargs):
            return mock_response

        # Mock asyncio.to_thread to return mock response
        with patch("asyncio.to_thread", side_effect=mock_invoke):
            # Create client with Bedrock provider
            # model=None is allowed for Bedrock (Issue #1 fix)
            client = LLMClient(
                endpoint="http://dummy",  # Currently required even for Bedrock
                model=None,  # Optional for Bedrock
                provider="bedrock",
                bedrock_region="us-east-1",
                bedrock_model="anthropic.claude-sonnet-4-5-20250929-v1:0",
            )

            # CRITICAL: client.model should be set to bedrock_model
            assert client.model == "anthropic.claude-sonnet-4-5-20250929-v1:0", (
                "client.model should be set to bedrock_model for cache keys and metadata"
            )

            # Verify cache would use correct model name
            with TemporaryDirectory() as temp_dir:
                cache = IntelligentCache(cache_dir=Path(temp_dir), ttl_days=30)
                client.cache = cache

                response = await client.analyze_code(system_prompt="Test", code="def foo(): pass")

                # Verify response has correct model name
                assert response.model == "anthropic.claude-sonnet-4-5-20250929-v1:0", (
                    "LLMResponse.model should contain actual Bedrock model name"
                )


@pytest.mark.asyncio
async def test_llm_client_bedrock_allows_none_endpoint():
    """Test LLMClient handles endpoint=None for Bedrock provider (bonus issue)."""
    from unittest.mock import MagicMock, patch

    from drep.llm.client import LLMClient

    with patch("boto3.client") as mock_boto_client:
        mock_bedrock = MagicMock()
        mock_boto_client.return_value = mock_bedrock

        # This should work - Bedrock doesn't need endpoint
        client = LLMClient(
            endpoint=None,  # Should be allowed for Bedrock
            model=None,
            provider="bedrock",
            bedrock_region="us-west-2",
            bedrock_model="anthropic.claude-haiku-4-5-20251001-v1:0",
        )

        assert client._provider == "bedrock"
        assert client.bedrock_client.model == "anthropic.claude-haiku-4-5-20251001-v1:0"


class TestCircuitBreakerWiring:
    """C23: LLMClient wraps the provider transport call in its CircuitBreaker."""

    @staticmethod
    def _failing_transport():
        calls = {"n": 0}

        async def create(**kwargs):
            calls["n"] += 1
            raise RuntimeError("transport down")

        return calls, create

    @staticmethod
    def _ok_transport():
        calls = {"n": 0}

        from types import SimpleNamespace

        ok_resp = SimpleNamespace(
            choices=[SimpleNamespace(message=SimpleNamespace(content="ok"))],
            usage=SimpleNamespace(total_tokens=10, prompt_tokens=4, completion_tokens=6),
            model="test-model",
        )

        async def create(**kwargs):
            calls["n"] += 1
            return ok_resp

        return calls, create

    @pytest.mark.asyncio
    async def test_consecutive_failures_open_circuit(self):
        """After threshold transport failures the breaker opens and short-circuits."""
        from drep.llm.circuit_breaker import CircuitBreaker, CircuitBreakerOpenError
        from drep.llm.client import LLMClient

        calls, create = self._failing_transport()
        client = LLMClient(
            endpoint="http://localhost:1234/v1",
            model="m",
            max_retries=1,
            circuit_breaker=CircuitBreaker(failure_threshold=2, recovery_timeout=60),
        )
        client.client = _FakeChat(create)

        for _ in range(2):
            with pytest.raises(RuntimeError, match="transport down"):
                await client.analyze_code("sys", "code")
        assert calls["n"] == 2

        # Circuit open: next call fails fast WITHOUT invoking the transport
        with pytest.raises(CircuitBreakerOpenError):
            await client.analyze_code("sys", "code")
        assert calls["n"] == 2

    @pytest.mark.asyncio
    async def test_zero_retries_still_attempts_once(self):
        """max_retries=0 means "no retries", not "never send the request".

        Config allows 0 (ge=0). Skipping the loop entirely would raise a
        misleading "no exception was captured" RuntimeError that hides the real
        transport error from whoever is reading a failed pre-commit run.
        """
        from drep.llm.client import LLMClient

        calls, create = self._failing_transport()
        client = LLMClient(endpoint="http://localhost:1234/v1", model="m", max_retries=0)
        client.client = _FakeChat(create)

        with pytest.raises(RuntimeError, match="transport down"):
            await client.analyze_code("sys", "code")
        assert calls["n"] == 1

    @pytest.mark.asyncio
    async def test_disabled_breaker_leaves_transport_unwrapped(self):
        """enable_circuit_breaker=False keeps direct transport calls."""
        from drep.llm.client import LLMClient

        calls, create = self._ok_transport()
        client = LLMClient(
            endpoint="http://localhost:1234/v1",
            model="m",
            enable_circuit_breaker=False,
        )
        client.client = _FakeChat(create)

        resp = await client.analyze_code("sys", "code")
        assert resp.content == "ok"
        assert calls["n"] == 1


class _FakeChat:
    def __init__(self, create):
        self.chat = type(
            "Chat", (), {"completions": type("Completions", (), {"create": staticmethod(create)})()}
        )()


class TestEmptyContentResponses:
    """A response with no content is a failed request, not a TypeError.

    Reasoning models can spend the whole token budget on `reasoning` and return
    `content: null`; refusals do the same. That used to surface four frames
    deep as "argument of type 'NoneType' is not a container or iterable".
    """

    @staticmethod
    def _transport(content, finish_reason="stop"):
        async def create(**kwargs):
            message = type("Message", (), {"content": content})()
            choice = type("Choice", (), {"message": message, "finish_reason": finish_reason})()
            usage = type(
                "Usage", (), {"total_tokens": 5, "prompt_tokens": 3, "completion_tokens": 2}
            )()
            return type("Response", (), {"choices": [choice], "usage": usage, "model": "m"})()

        return create

    @pytest.mark.asyncio
    async def test_none_content_raises_a_diagnostic_error(self):
        from drep.llm.client import LLMClient

        client = LLMClient(endpoint="http://localhost:1234/v1", model="m", max_retries=1)
        client.client = _FakeChat(self._transport(None, finish_reason="length"))

        with pytest.raises(ValueError, match="no content"):
            await client.analyze_code("sys", "code")

    @pytest.mark.asyncio
    async def test_error_names_the_finish_reason(self):
        """'length' is the actionable case - raise max_tokens or drop the reasoning model."""
        from drep.llm.client import LLMClient

        client = LLMClient(endpoint="http://localhost:1234/v1", model="m", max_retries=1)
        client.client = _FakeChat(self._transport(None, finish_reason="length"))

        with pytest.raises(ValueError, match="length"):
            await client.analyze_code("sys", "code")

    @pytest.mark.asyncio
    async def test_normal_content_still_returns(self):
        from drep.llm.client import LLMClient

        client = LLMClient(endpoint="http://localhost:1234/v1", model="m", max_retries=1)
        client.client = _FakeChat(self._transport("hello"))

        response = await client.analyze_code("sys", "code")
        assert response.content == "hello"

    @pytest.mark.asyncio
    async def test_cached_empty_content_is_treated_as_a_miss(self):
        """A poisoned cache entry must not replay the crash forever.

        The first bad response was written to the cache before the parser
        choked on it, so every later run replayed content=None from cache and
        failed in 0.6s without ever calling the LLM again.
        """
        from unittest.mock import MagicMock

        from drep.llm.client import LLMClient

        cache = MagicMock()
        cache.get.return_value = {
            "content": None,
            "tokens_used": 5,
            "latency_ms": 1,
            "model": "m",
        }
        client = LLMClient(
            endpoint="http://localhost:1234/v1", model="m", max_retries=1, cache=cache
        )
        client.client = _FakeChat(self._transport("fresh"))

        response = await client.analyze_code("sys", "code")

        # Refetched rather than replayed
        assert response.content == "fresh"
