"""AWS Bedrock provider for LLM client.

Authentication:
    Uses boto3's standard credential chain in order:
    1. Environment variables (AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY, AWS_SESSION_TOKEN)
    2. Shared credentials file (~/.aws/credentials)
    3. AWS config file (~/.aws/config)
    4. Container credentials (ECS tasks)
    5. Instance metadata service (EC2 IAM roles)

    For more details: https://boto3.amazonaws.com/v1/documentation/api/latest/guide/credentials.html
"""

import asyncio
import json
import logging
from typing import Any, Dict, List, Optional, Tuple

import boto3
from botocore.exceptions import (
    ClientError,
    EndpointConnectionError,
    NoCredentialsError,
    PartialCredentialsError,
)

logger = logging.getLogger(__name__)

# User-friendly error messages for common AWS Bedrock errors
ERROR_MESSAGES = {
    "ThrottlingException": (
        "AWS Bedrock rate limit exceeded. " "Reduce requests_per_minute or wait before retrying."
    ),
    "AccessDeniedException": (
        "AWS Bedrock access denied. "
        "Check IAM permissions and enable model access in AWS Console."
    ),
    "ValidationException": (
        "Invalid request parameters. " "Verify model ID format and region availability."
    ),
    "ResourceNotFoundException": (
        "Model not found in this region. " "Check model ID and regional availability."
    ),
    "ServiceQuotaExceededException": (
        "AWS service quota exceeded. Request quota increase or reduce usage."
    ),
}


class BedrockClient:
    """AWS Bedrock client for Claude models.

    Provides OpenAI-compatible interface for AWS Bedrock Claude models.
    Uses AWS credentials chain (env vars, ~/.aws/credentials, IAM roles).
    """

    def __init__(
        self,
        region: str = "us-east-1",
        model: str = "anthropic.claude-sonnet-4-5-20250929-v1:0",
    ):
        """Initialize Bedrock client.

        Args:
            region: AWS region (default: us-east-1)
            model: Bedrock model ID (default: Claude Sonnet 4.5)

        Raises:
            ValueError: If AWS credentials are missing, incomplete, or endpoint unreachable
        """
        self.region = region
        self.model = model

        # Initialize boto3 bedrock-runtime client using AWS credentials chain
        try:
            self.bedrock_client = boto3.client(
                service_name="bedrock-runtime",
                region_name=region,
            )
            logger.info(f"Successfully initialized Bedrock client: region={region}, model={model}")

        except NoCredentialsError as e:
            logger.error(
                "AWS credentials not found. Please configure credentials via:\n"
                "  1. Environment variables: AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY\n"
                "  2. AWS credentials file: ~/.aws/credentials\n"
                "  3. IAM role (if running on EC2/ECS/Lambda)"
            )
            raise ValueError(
                "AWS Bedrock requires credentials. "
                "See https://docs.aws.amazon.com/cli/latest/userguide/cli-configure-files.html"
            ) from e

        except PartialCredentialsError as e:
            logger.error("AWS credentials are incomplete")
            raise ValueError(
                "Incomplete AWS credentials. "
                "Both AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY are required."
            ) from e

        except EndpointConnectionError as e:
            logger.error(f"Cannot connect to AWS Bedrock in region {region}")
            raise ValueError(
                f"Cannot connect to AWS Bedrock in region {region}. "
                f"Check your network connection and verify Bedrock is available in this region."
            ) from e

    def _format_messages(
        self, messages: List[Dict[str, str]]
    ) -> Tuple[List[Dict[str, Any]], Optional[str]]:
        """Convert OpenAI message format to Bedrock format.

        Args:
            messages: OpenAI-style messages [{"role": "user", "content": "..."}]

        Returns:
            Tuple of (bedrock_messages, system_prompt)
            - bedrock_messages: Messages in Bedrock format
            - system_prompt: Extracted system prompt (or None)

        Notes:
            - System prompts are extracted and combined into separate field
            - Content is wrapped in [{"type": "text", "text": "..."}] format
        """
        bedrock_messages = []
        system_prompts = []

        for msg in messages:
            role = msg["role"]
            content = msg["content"]

            if role == "system":
                # Extract system prompts
                system_prompts.append(content)
            else:
                # Convert to Bedrock message format
                bedrock_messages.append(
                    {"role": role, "content": [{"type": "text", "text": content}]}
                )

        # Combine system prompts
        system_prompt = "\n\n".join(system_prompts) if system_prompts else None

        return bedrock_messages, system_prompt

    def _parse_response(self, bedrock_response: Dict[str, Any]) -> Dict[str, Any]:
        """Convert Bedrock response to OpenAI-compatible format.

        Args:
            bedrock_response: Raw Bedrock response

        Returns:
            OpenAI-compatible response dict with:
            - choices[0].message.content: Response text
            - usage.prompt_tokens: Input tokens
            - usage.completion_tokens: Output tokens
            - usage.total_tokens: Total tokens
        """
        # Extract content
        content_blocks = bedrock_response.get("content", [])

        if not content_blocks:
            logger.warning(
                f"Bedrock returned empty content array. "
                f"This may indicate model refusal, quota exhaustion, or configuration issues. "
                f"Response keys: {list(bedrock_response.keys())}"
            )
            text_content = ""
        else:
            # Filter text blocks
            text_blocks = [
                block.get("text", "") for block in content_blocks if block.get("type") == "text"
            ]

            if not text_blocks:
                logger.warning(
                    f"Bedrock returned content blocks but none were text type. "
                    f"Content block types: {[block.get('type') for block in content_blocks]}"
                )

            text_content = " ".join(text_blocks)

        # Extract token usage
        usage = bedrock_response.get("usage", {})
        prompt_tokens = usage.get("input_tokens", 0)
        completion_tokens = usage.get("output_tokens", 0)

        # Build OpenAI-compatible response
        return {
            "choices": [
                {
                    "message": {
                        "role": "assistant",
                        "content": text_content,
                    },
                    "finish_reason": "stop",
                }
            ],
            "usage": {
                "prompt_tokens": prompt_tokens,
                "completion_tokens": completion_tokens,
                "total_tokens": prompt_tokens + completion_tokens,
            },
        }

    async def chat_completion(
        self,
        messages: List[Dict[str, str]],
        max_tokens: int = 4000,
        temperature: float = 0.2,
        **kwargs,
    ) -> Dict[str, Any]:
        """Execute chat completion request via Bedrock.

        Args:
            messages: OpenAI-style messages
            max_tokens: Maximum tokens to generate
            temperature: Sampling temperature (0.0-1.0)
            **kwargs: Additional parameters (ignored for Bedrock)

        Returns:
            OpenAI-compatible response dict

        Raises:
            Exception: For Bedrock errors (throttling, access denied, validation, etc.)
        """
        # Convert messages to Bedrock format
        bedrock_messages, system_prompt = self._format_messages(messages)

        # Build Bedrock request body
        # NOTE: anthropic_version "bedrock-2023-05-31" is REQUIRED by AWS Bedrock API
        # for all Claude models. This is AWS's schema version, distinct from model version.
        # Do NOT change without consulting AWS documentation:
        # https://docs.aws.amazon.com/bedrock/latest/userguide/model-parameters-anthropic-claude-messages.html
        body = {
            "anthropic_version": "bedrock-2023-05-31",
            "max_tokens": max_tokens,
            "messages": bedrock_messages,
            "temperature": temperature,
        }

        # Add system prompt if present
        if system_prompt:
            body["system"] = system_prompt

        try:
            # Call Bedrock invoke_model in thread pool to avoid blocking event loop
            # boto3 is synchronous, so we must use asyncio.to_thread to prevent
            # blocking other async tasks (rate limiting, progress tracking, etc.)
            response = await asyncio.to_thread(
                self.bedrock_client.invoke_model,
                modelId=self.model,
                contentType="application/json",
                accept="application/json",
                body=json.dumps(body).encode("utf-8"),  # AWS requires bytes, not string
            )

            # Read and parse response body with proper resource management
            try:
                body_stream = response["body"]
                raw_body = body_stream.read()
                logger.debug(f"Bedrock raw response size: {len(raw_body)} bytes")
            except KeyError:
                logger.error("Bedrock response missing 'body' field")
                raise ValueError("Invalid Bedrock response: missing 'body' field")
            finally:
                # Only close if body_stream was successfully assigned.
                # KeyError on response["body"] means it never enters locals(),
                # preventing NameError on body_stream.close()
                if "body_stream" in locals():
                    body_stream.close()  # Always close the StreamingBody

            # Parse JSON with validation
            try:
                response_body = json.loads(raw_body)
                logger.debug("Successfully parsed Bedrock JSON response")
            except json.JSONDecodeError as e:
                preview = (
                    raw_body[:500].decode("utf-8", errors="replace")
                    if isinstance(raw_body, bytes)
                    else str(raw_body)[:500]
                )
                logger.error(f"Bedrock returned invalid JSON: {e}\nResponse preview: {preview}")
                raise ValueError(
                    f"Bedrock returned invalid JSON response. "
                    f"AWS service issue suspected. Error: {e}"
                ) from e

            # Convert to OpenAI format
            return self._parse_response(response_body)

        except ClientError as e:
            error_code = e.response.get("Error", {}).get("Code", "Unknown")
            error_message = e.response.get("Error", {}).get("Message", str(e))

            # Log error with details
            logger.error(f"Bedrock API error: code={error_code}, message={error_message}")

            # Provide user-friendly error message
            user_message = ERROR_MESSAGES.get(error_code, error_message)
            raise Exception(f"Bedrock request failed: {user_message}") from e

        except (KeyError, AttributeError) as e:
            # Response structure errors
            logger.error(f"Bedrock response structure error: {type(e).__name__}: {e}")
            raise ValueError(f"Unexpected Bedrock response structure: {e}") from e

        except Exception as e:
            # Unexpected errors - log with full traceback
            logger.error(
                f"Unexpected error in Bedrock request: {type(e).__name__}: {e}", exc_info=True
            )
            raise

    async def close(self):
        """Close Bedrock client (no-op for boto3)."""
        # boto3 clients don't require explicit cleanup
        pass
