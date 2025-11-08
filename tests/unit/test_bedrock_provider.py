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

    # Should preserve error details
    assert "ThrottlingException" in str(exc_info.value) or "Rate exceeded" in str(exc_info.value)


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

    assert "AccessDeniedException" in str(exc_info.value) or "Not authorized" in str(exc_info.value)


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

    assert "ValidationException" in str(exc_info.value) or "Invalid parameters" in str(
        exc_info.value
    )


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

    # Multiple system prompts should be combined
    assert "Be concise" in system_prompt
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

    # Get the call args
    call_args = mock_bedrock.invoke_model.call_args

    # Verify body parameter exists and has correct structure
    body_str = call_args.kwargs.get("body") or call_args[1].get("body")
    body = json.loads(body_str)

    # Check required Bedrock fields
    assert body["anthropic_version"] == "bedrock-2023-05-31"
    assert body["max_tokens"] == 2000
    assert body["system"] == "You are helpful."
    assert "messages" in body
    assert len(body["messages"]) == 1
    assert body["messages"][0]["role"] == "user"
