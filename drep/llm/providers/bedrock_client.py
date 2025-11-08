"""AWS Bedrock provider for LLM client."""

import json
import logging
from typing import Any, Dict, List, Optional, Tuple

import boto3
from botocore.exceptions import ClientError

logger = logging.getLogger(__name__)


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
        """
        self.region = region
        self.model = model

        # Initialize boto3 bedrock-runtime client using AWS credentials chain
        self.bedrock_client = boto3.client(
            service_name="bedrock-runtime",
            region_name=region,
        )

        logger.info(f"Initialized Bedrock client: region={region}, model={model}")

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
        if content_blocks:
            # Combine all text blocks
            text_content = " ".join(
                block.get("text", "") for block in content_blocks if block.get("type") == "text"
            )
        else:
            text_content = ""

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
            # Call Bedrock invoke_model
            response = self.bedrock_client.invoke_model(
                modelId=self.model,
                body=json.dumps(body),
            )

            # Parse response
            response_body = json.loads(response["body"].read())

            # Convert to OpenAI format
            return self._parse_response(response_body)

        except ClientError as e:
            error_code = e.response.get("Error", {}).get("Code", "Unknown")
            error_message = e.response.get("Error", {}).get("Message", str(e))

            # Log error with details
            logger.error(f"Bedrock API error: code={error_code}, message={error_message}")

            # Wrap and re-raise with context
            raise Exception(f"Bedrock API error ({error_code}): {error_message}") from e

        except Exception as e:
            # Log and re-raise generic errors
            logger.error(f"Bedrock request failed: {e}")
            raise

    async def close(self):
        """Close Bedrock client (no-op for boto3)."""
        # boto3 clients don't require explicit cleanup
        pass
