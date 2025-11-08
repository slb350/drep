"""Unit tests for AWS Bedrock provider."""

import json
from unittest.mock import MagicMock, patch

import pytest


@pytest.mark.asyncio
async def test_bedrock_client_initialization_default_region():
    """Test BedrockClient initializes with default region."""
    from drep.llm.providers.bedrock_client import BedrockClient

    client = BedrockClient(
        region="us-east-1",
        model="anthropic.claude-sonnet-4-5-20250929-v1:0",
    )

    assert client.region == "us-east-1"
    assert client.model == "anthropic.claude-sonnet-4-5-20250929-v1:0"
    assert client.bedrock_client is not None


@pytest.mark.asyncio
async def test_bedrock_client_initialization_custom_region():
    """Test BedrockClient initializes with custom region."""
    from drep.llm.providers.bedrock_client import BedrockClient

    client = BedrockClient(
        region="us-west-2",
        model="anthropic.claude-haiku-4-5-20251001-v1:0",
    )

    assert client.region == "us-west-2"
    assert client.model == "anthropic.claude-haiku-4-5-20251001-v1:0"


@pytest.mark.asyncio
async def test_bedrock_client_message_format_conversion():
    """Test conversion from OpenAI format to Bedrock format."""
    from drep.llm.providers.bedrock_client import BedrockClient

    client = BedrockClient(
        region="us-east-1",
        model="anthropic.claude-sonnet-4-5-20250929-v1:0",
    )

    openai_messages = [
        {"role": "system", "content": "You are a helpful assistant."},
        {"role": "user", "content": "Hello, how are you?"},
    ]

    bedrock_messages, system_prompt = client._format_messages(openai_messages)

    # System prompt should be extracted
    assert system_prompt == "You are a helpful assistant."

    # Messages should only contain user message
    assert len(bedrock_messages) == 1
    assert bedrock_messages[0]["role"] == "user"
    assert bedrock_messages[0]["content"] == [{"type": "text", "text": "Hello, how are you?"}]


@pytest.mark.asyncio
async def test_bedrock_client_message_format_no_system():
    """Test message formatting without system prompt."""
    from drep.llm.providers.bedrock_client import BedrockClient

    client = BedrockClient(
        region="us-east-1",
        model="anthropic.claude-sonnet-4-5-20250929-v1:0",
    )

    openai_messages = [
        {"role": "user", "content": "Hello!"},
        {"role": "assistant", "content": "Hi there!"},
        {"role": "user", "content": "How are you?"},
    ]

    bedrock_messages, system_prompt = client._format_messages(openai_messages)

    # No system prompt
    assert system_prompt is None

    # All messages should be converted
    assert len(bedrock_messages) == 3
    assert bedrock_messages[0]["role"] == "user"
    assert bedrock_messages[1]["role"] == "assistant"
    assert bedrock_messages[2]["role"] == "user"


@pytest.mark.asyncio
async def test_bedrock_client_response_parsing():
    """Test parsing Bedrock response to OpenAI format."""
    from drep.llm.providers.bedrock_client import BedrockClient

    client = BedrockClient(
        region="us-east-1",
        model="anthropic.claude-sonnet-4-5-20250929-v1:0",
    )

    bedrock_response = {
        "content": [{"type": "text", "text": "This is a test response."}],
        "usage": {
            "input_tokens": 100,
            "output_tokens": 50,
        },
    }

    openai_response = client._parse_response(bedrock_response)

    # Check OpenAI-compatible structure
    assert "choices" in openai_response
    assert len(openai_response["choices"]) == 1
    assert openai_response["choices"][0]["message"]["role"] == "assistant"
    assert openai_response["choices"][0]["message"]["content"] == "This is a test response."

    # Check token mapping
    assert "usage" in openai_response
    assert openai_response["usage"]["prompt_tokens"] == 100
    assert openai_response["usage"]["completion_tokens"] == 50
    assert openai_response["usage"]["total_tokens"] == 150


@pytest.mark.asyncio
@patch("boto3.client")
async def test_bedrock_client_chat_completion_success(mock_boto_client):
    """Test successful chat completion call."""
    from drep.llm.providers.bedrock_client import BedrockClient

    # Mock boto3 response
    mock_bedrock = MagicMock()
    mock_boto_client.return_value = mock_bedrock

    mock_body = json.dumps(
        {
            "content": [{"type": "text", "text": "Hello from Bedrock!"}],
            "usage": {"input_tokens": 50, "output_tokens": 20},
        }
    ).encode("utf-8")

    mock_response = {"body": MagicMock(read=MagicMock(return_value=mock_body))}
    mock_bedrock.invoke_model = MagicMock(return_value=mock_response)

    client = BedrockClient(
        region="us-east-1",
        model="anthropic.claude-sonnet-4-5-20250929-v1:0",
    )

    messages = [{"role": "user", "content": "Hello!"}]
    response = await client.chat_completion(messages, max_tokens=1000, temperature=0.7)

    # Verify response structure
    assert response["choices"][0]["message"]["content"] == "Hello from Bedrock!"
    assert response["usage"]["prompt_tokens"] == 50
    assert response["usage"]["completion_tokens"] == 20


@pytest.mark.asyncio
@patch("boto3.client")
async def test_bedrock_client_throttling_error(mock_boto_client):
    """Test handling of ThrottlingException."""
    from botocore.exceptions import ClientError

    from drep.llm.providers.bedrock_client import BedrockClient

    mock_bedrock = MagicMock()
    mock_boto_client.return_value = mock_bedrock

    # Simulate ThrottlingException
    error_response = {"Error": {"Code": "ThrottlingException", "Message": "Rate exceeded"}}
    mock_bedrock.invoke_model.side_effect = ClientError(error_response, "invoke_model")

    client = BedrockClient(
        region="us-east-1",
        model="anthropic.claude-sonnet-4-5-20250929-v1:0",
    )

    messages = [{"role": "user", "content": "Test"}]

    with pytest.raises(Exception) as exc_info:
        await client.chat_completion(messages)

    # Should show user-friendly error message
    assert "rate limit exceeded" in str(exc_info.value).lower()


@pytest.mark.asyncio
@patch("boto3.client")
async def test_bedrock_client_access_denied_error(mock_boto_client):
    """Test handling of AccessDeniedException."""
    from botocore.exceptions import ClientError

    from drep.llm.providers.bedrock_client import BedrockClient

    mock_bedrock = MagicMock()
    mock_boto_client.return_value = mock_bedrock

    # Simulate AccessDeniedException
    error_response = {"Error": {"Code": "AccessDeniedException", "Message": "Not authorized"}}
    mock_bedrock.invoke_model.side_effect = ClientError(error_response, "invoke_model")

    client = BedrockClient(
        region="us-east-1",
        model="anthropic.claude-sonnet-4-5-20250929-v1:0",
    )

    messages = [{"role": "user", "content": "Test"}]

    with pytest.raises(Exception) as exc_info:
        await client.chat_completion(messages)

    # Should show user-friendly error message
    assert "access denied" in str(exc_info.value).lower()


@pytest.mark.asyncio
@patch("boto3.client")
async def test_bedrock_client_validation_error(mock_boto_client):
    """Test handling of ValidationException."""
    from botocore.exceptions import ClientError

    from drep.llm.providers.bedrock_client import BedrockClient

    mock_bedrock = MagicMock()
    mock_boto_client.return_value = mock_bedrock

    # Simulate ValidationException
    error_response = {"Error": {"Code": "ValidationException", "Message": "Invalid parameters"}}
    mock_bedrock.invoke_model.side_effect = ClientError(error_response, "invoke_model")

    client = BedrockClient(
        region="us-east-1",
        model="anthropic.claude-sonnet-4-5-20250929-v1:0",
    )

    messages = [{"role": "user", "content": "Test"}]

    with pytest.raises(Exception) as exc_info:
        await client.chat_completion(messages)

    # Should show user-friendly error message
    assert "invalid request parameters" in str(exc_info.value).lower()


@pytest.mark.asyncio
@patch("boto3.client")
async def test_bedrock_client_generic_error(mock_boto_client):
    """Test handling of generic boto3 errors."""
    from drep.llm.providers.bedrock_client import BedrockClient

    mock_bedrock = MagicMock()
    mock_boto_client.return_value = mock_bedrock

    # Simulate generic exception
    mock_bedrock.invoke_model.side_effect = Exception("Unknown error")

    client = BedrockClient(
        region="us-east-1",
        model="anthropic.claude-sonnet-4-5-20250929-v1:0",
    )

    messages = [{"role": "user", "content": "Test"}]

    with pytest.raises(Exception) as exc_info:
        await client.chat_completion(messages)

    assert "Unknown error" in str(exc_info.value)


@pytest.mark.asyncio
async def test_bedrock_client_close():
    """Test BedrockClient close method."""
    from drep.llm.providers.bedrock_client import BedrockClient

    client = BedrockClient(
        region="us-east-1",
        model="anthropic.claude-sonnet-4-5-20250929-v1:0",
    )

    # Close should not raise any errors
    await client.close()


@pytest.mark.asyncio
async def test_bedrock_client_global_model_id():
    """Test BedrockClient with global model ID format."""
    from drep.llm.providers.bedrock_client import BedrockClient

    client = BedrockClient(
        region="us-west-2",
        model="global.anthropic.claude-sonnet-4-5-20250929-v1:0",
    )

    assert client.model == "global.anthropic.claude-sonnet-4-5-20250929-v1:0"


@pytest.mark.asyncio
async def test_bedrock_client_system_prompt_extraction():
    """Test that system prompts are properly extracted from messages."""
    from drep.llm.providers.bedrock_client import BedrockClient

    client = BedrockClient(
        region="us-east-1",
        model="anthropic.claude-sonnet-4-5-20250929-v1:0",
    )

    messages = [
        {"role": "system", "content": "Be concise."},
        {"role": "user", "content": "Hello"},
        {"role": "system", "content": "This should also be extracted"},
    ]

    bedrock_messages, system_prompt = client._format_messages(messages)

    # Multiple system prompts should be combined with newlines
    expected_system = "Be concise.\n\nThis should also be extracted"
    assert (
        system_prompt == expected_system
    ), f"Expected: {expected_system!r}, Got: {system_prompt!r}"
    assert len([m for m in bedrock_messages if m["role"] == "user"]) == 1


@pytest.mark.asyncio
@patch("boto3.client")
async def test_bedrock_client_empty_response_handling(mock_boto_client):
    """Test handling of empty/malformed responses."""
    from drep.llm.providers.bedrock_client import BedrockClient

    mock_bedrock = MagicMock()
    mock_boto_client.return_value = mock_bedrock

    # Mock empty response
    mock_body = json.dumps(
        {
            "content": [],
            "usage": {"input_tokens": 10, "output_tokens": 0},
        }
    ).encode("utf-8")

    mock_response = {"body": MagicMock(read=MagicMock(return_value=mock_body))}
    mock_bedrock.invoke_model = MagicMock(return_value=mock_response)

    client = BedrockClient(
        region="us-east-1",
        model="anthropic.claude-sonnet-4-5-20250929-v1:0",
    )

    messages = [{"role": "user", "content": "Test"}]
    response = await client.chat_completion(messages)

    # Should handle empty content gracefully
    assert response["choices"][0]["message"]["content"] == ""


@pytest.mark.asyncio
@patch("boto3.client")
async def test_bedrock_client_request_body_format(mock_boto_client):
    """Test that request body has correct Bedrock format."""
    from drep.llm.providers.bedrock_client import BedrockClient

    mock_bedrock = MagicMock()
    mock_boto_client.return_value = mock_bedrock

    mock_body = json.dumps(
        {
            "content": [{"type": "text", "text": "Response"}],
            "usage": {"input_tokens": 50, "output_tokens": 20},
        }
    ).encode("utf-8")

    mock_response = {"body": MagicMock(read=MagicMock(return_value=mock_body))}
    mock_bedrock.invoke_model = MagicMock(return_value=mock_response)

    client = BedrockClient(
        region="us-east-1",
        model="anthropic.claude-sonnet-4-5-20250929-v1:0",
    )

    messages = [
        {"role": "system", "content": "You are helpful."},
        {"role": "user", "content": "Hello"},
    ]

    await client.chat_completion(messages, max_tokens=2000, temperature=0.5)

    # Verify invoke_model was called
    assert mock_bedrock.invoke_model.called


@pytest.mark.asyncio
@patch("boto3.client")
async def test_bedrock_client_closes_streaming_body(mock_boto_client):
    """Test that StreamingBody is properly closed after reading (Issue #1 from PR review)."""
    from drep.llm.providers.bedrock_client import BedrockClient

    mock_bedrock = MagicMock()
    mock_boto_client.return_value = mock_bedrock

    # Create tracked mock with close() to verify it's called
    mock_stream = MagicMock()
    mock_body = json.dumps(
        {
            "content": [{"type": "text", "text": "Response"}],
            "usage": {"input_tokens": 10, "output_tokens": 5},
        }
    ).encode("utf-8")
    mock_stream.read = MagicMock(return_value=mock_body)
    mock_stream.close = MagicMock()

    mock_response = {"body": mock_stream}
    mock_bedrock.invoke_model = MagicMock(return_value=mock_response)

    client = BedrockClient(
        region="us-east-1",
        model="anthropic.claude-sonnet-4-5-20250929-v1:0",
    )

    messages = [{"role": "user", "content": "Test"}]

    await client.chat_completion(messages)

    # Verify close() was called exactly once
    mock_stream.close.assert_called_once()


@pytest.mark.asyncio
@patch("boto3.client")
async def test_bedrock_client_closes_streaming_body_on_error(mock_boto_client):
    """Test StreamingBody.close() called even on JSON parse error."""
    from drep.llm.providers.bedrock_client import BedrockClient

    mock_bedrock = MagicMock()
    mock_boto_client.return_value = mock_bedrock

    # Create mock stream that returns invalid JSON
    mock_stream = MagicMock()
    mock_stream.read = MagicMock(return_value=b"invalid json {{{")
    mock_stream.close = MagicMock()

    mock_response = {"body": mock_stream}
    mock_bedrock.invoke_model = MagicMock(return_value=mock_response)

    client = BedrockClient(
        region="us-east-1",
        model="anthropic.claude-sonnet-4-5-20250929-v1:0",
    )

    messages = [{"role": "user", "content": "Test"}]

    # Should raise ValueError due to invalid JSON
    with pytest.raises(ValueError, match="invalid JSON"):
        await client.chat_completion(messages)

    # Verify close() was still called despite error
    mock_stream.close.assert_called_once()


@pytest.mark.asyncio
@patch("boto3.client")
@patch("asyncio.to_thread")
async def test_bedrock_client_uses_asyncio_to_thread(mock_to_thread, mock_boto_client):
    """Test BedrockClient uses asyncio.to_thread to avoid blocking event loop (Issue #2)."""
    from drep.llm.providers.bedrock_client import BedrockClient

    mock_bedrock = MagicMock()
    mock_boto_client.return_value = mock_bedrock

    # Mock successful response
    mock_body = json.dumps(
        {
            "content": [{"type": "text", "text": "Non-blocking response"}],
            "usage": {"input_tokens": 10, "output_tokens": 5},
        }
    ).encode("utf-8")

    mock_response = {"body": MagicMock(read=MagicMock(return_value=mock_body), close=MagicMock())}

    # Mock asyncio.to_thread to return the mock response
    async def mock_async_invoke(*args, **kwargs):
        return mock_response

    mock_to_thread.side_effect = mock_async_invoke

    client = BedrockClient(
        region="us-east-1",
        model="anthropic.claude-sonnet-4-5-20250929-v1:0",
    )

    messages = [{"role": "user", "content": "Test"}]
    await client.chat_completion(messages)

    # Verify asyncio.to_thread was called (not direct invoke_model)
    mock_to_thread.assert_called_once()
    # Verify first arg to to_thread is the invoke_model method
    call_args = mock_to_thread.call_args
    assert call_args[0][0] == mock_bedrock.invoke_model


@pytest.mark.asyncio
@patch("boto3.client")
@patch("asyncio.to_thread")
async def test_bedrock_client_invoke_model_parameters(mock_to_thread, mock_boto_client):
    """Test invoke_model called with correct parameters (P1: contentType, accept, bytes body)."""
    from drep.llm.providers.bedrock_client import BedrockClient

    mock_bedrock = MagicMock()
    mock_boto_client.return_value = mock_bedrock

    # Mock successful response
    mock_body = json.dumps(
        {
            "content": [{"type": "text", "text": "Response"}],
            "usage": {"input_tokens": 10, "output_tokens": 5},
        }
    ).encode("utf-8")

    mock_response = {"body": MagicMock(read=MagicMock(return_value=mock_body), close=MagicMock())}

    # Mock asyncio.to_thread to return the mock response
    async def mock_async_invoke(*args, **kwargs):
        return mock_response

    mock_to_thread.side_effect = mock_async_invoke

    client = BedrockClient(
        region="us-east-1",
        model="anthropic.claude-sonnet-4-5-20250929-v1:0",
    )

    messages = [{"role": "user", "content": "Test"}]
    await client.chat_completion(messages, max_tokens=1000, temperature=0.5)

    # Verify asyncio.to_thread was called with correct parameters
    mock_to_thread.assert_called_once()

    # Extract the kwargs passed to invoke_model
    call_args = mock_to_thread.call_args
    # call_args[1] contains the kwargs passed to invoke_model
    invoke_kwargs = call_args[1] if len(call_args) > 1 else {}

    # Verify modelId
    assert "modelId" in invoke_kwargs
    assert invoke_kwargs["modelId"] == "anthropic.claude-sonnet-4-5-20250929-v1:0"

    # Verify contentType is set (AWS best practice)
    assert "contentType" in invoke_kwargs
    assert invoke_kwargs["contentType"] == "application/json"

    # Verify accept is set (to receive JSON response)
    assert "accept" in invoke_kwargs
    assert invoke_kwargs["accept"] == "application/json"

    # Verify body is bytes, not string (AWS API requirement)
    assert "body" in invoke_kwargs
    assert isinstance(invoke_kwargs["body"], bytes), "Body must be bytes, not string"

    # Verify body contains valid JSON
    body_dict = json.loads(invoke_kwargs["body"].decode("utf-8"))
    assert "anthropic_version" in body_dict
    assert body_dict["anthropic_version"] == "bedrock-2023-05-31"
    assert body_dict["max_tokens"] == 1000
    assert body_dict["temperature"] == 0.5
