"""LLM client with rate limiting and robust JSON parsing."""

import asyncio
import json
import logging
import re
import subprocess
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Dict, Optional, Type

from openai import AsyncOpenAI
from pydantic import BaseModel

logger = logging.getLogger(__name__)


def get_current_commit_sha(repo_path: Optional[Path] = None) -> str:
    """Get current git commit SHA.

    Args:
        repo_path: Path to git repository (defaults to current directory)

    Returns:
        Commit SHA string, or "unknown" if not in a git repository

    Raises:
        RuntimeError: If git command fails unexpectedly
    """
    try:
        cwd = repo_path if repo_path else Path.cwd()
        result = subprocess.run(
            ["git", "rev-parse", "HEAD"],
            cwd=cwd,
            capture_output=True,
            text=True,
            timeout=5,
        )

        if result.returncode == 0:
            return result.stdout.strip()
        else:
            # Not a git repository or git not available
            logger.warning(f"Could not get commit SHA: {result.stderr}")
            return "unknown"

    except subprocess.TimeoutExpired:
        logger.warning("Git command timed out")
        return "unknown"
    except FileNotFoundError:
        logger.warning("Git not found in PATH")
        return "unknown"
    except Exception as e:
        logger.warning(f"Error getting commit SHA: {e}")
        return "unknown"


@dataclass
class LLMResponse:
    """Structured LLM response with metadata."""

    content: str
    tokens_used: int
    latency_ms: float
    model: str


class RateLimitContext:
    """Async context manager for rate-limited LLM requests.

    Holds the semaphore for the entire duration of the request to enforce
    concurrency limits properly.
    """

    def __init__(self, rate_limiter: "RateLimiter", estimated_tokens: int, repo_id: Optional[str]):
        """Initialize rate limit context.

        Args:
            rate_limiter: Parent RateLimiter instance
            estimated_tokens: Estimated tokens for this request
            repo_id: Optional repository identifier for per-repo limits
        """
        self.rate_limiter = rate_limiter
        self.estimated_tokens = estimated_tokens
        self.repo_id = repo_id
        self.actual_tokens: Optional[int] = None

    async def __aenter__(self):
        """Acquire semaphore and check rate limits."""
        # Acquire semaphore (held until __aexit__)
        await self.rate_limiter.semaphore.acquire()

        # Check request rate limit
        await self.rate_limiter._check_request_rate_limit()

        # Check token rate limit
        await self.rate_limiter._check_token_rate_limit(self.estimated_tokens)

        return self

    async def __aexit__(self, exc_type, exc_val, exc_tb):
        """Update token usage and release semaphore."""
        # Update actual token usage if set
        if self.actual_tokens is not None:
            async with self.rate_limiter.lock:
                # Adjust token count: remove estimate, add actual
                self.rate_limiter.tokens_used -= self.estimated_tokens
                self.rate_limiter.tokens_used += self.actual_tokens

        # Release semaphore
        self.rate_limiter.semaphore.release()

    def set_actual_tokens(self, tokens: int):
        """Set actual token usage after request completes.

        Args:
            tokens: Actual tokens used by the request
        """
        self.actual_tokens = tokens


class RateLimiter:
    """Dual-bucket rate limiter with concurrency control.

    Enforces:
    1. Maximum concurrent requests (semaphore)
    2. Requests per minute limit
    3. Tokens per minute limit
    """

    def __init__(
        self,
        max_concurrent: int,
        requests_per_minute: int,
        max_tokens_per_minute: int,
    ):
        """Initialize rate limiter.

        Args:
            max_concurrent: Maximum concurrent requests
            requests_per_minute: Request rate limit
            max_tokens_per_minute: Token rate limit
        """
        self.max_concurrent = max_concurrent
        self.requests_per_minute = requests_per_minute
        self.max_tokens_per_minute = max_tokens_per_minute

        # Concurrency control
        self.semaphore = asyncio.Semaphore(max_concurrent)

        # Request rate limiting
        self.lock = asyncio.Lock()
        self.request_times: list[float] = []

        # Token rate limiting
        self.tokens_used = 0
        self.token_reset_time = time.time() + 60

    async def _check_request_rate_limit(self):
        """Check and enforce request rate limit."""
        async with self.lock:
            now = time.time()

            # Remove requests older than 1 minute
            self.request_times = [t for t in self.request_times if now - t < 60]

            # Wait if at rate limit
            while len(self.request_times) >= self.requests_per_minute:
                oldest = self.request_times[0]
                wait_time = 60 - (now - oldest)
                if wait_time > 0:
                    logger.debug(f"Request rate limit reached, waiting {wait_time:.1f}s")
                    await asyncio.sleep(wait_time)

                # Refresh
                now = time.time()
                self.request_times = [t for t in self.request_times if now - t < 60]

            # Record this request
            self.request_times.append(now)

    async def _check_token_rate_limit(self, estimated_tokens: int):
        """Check and enforce token rate limit.

        Args:
            estimated_tokens: Estimated tokens for the request
        """
        async with self.lock:
            now = time.time()

            # Reset token counter if minute elapsed
            if now >= self.token_reset_time:
                self.tokens_used = 0
                self.token_reset_time = now + 60

            # Wait if adding this request would exceed limit
            while self.tokens_used + estimated_tokens > self.max_tokens_per_minute:
                wait_time = self.token_reset_time - now
                if wait_time > 0:
                    logger.debug(
                        f"Token rate limit reached ({self.tokens_used}/{self.max_tokens_per_minute}), "
                        f"waiting {wait_time:.1f}s"
                    )
                    await asyncio.sleep(wait_time)

                # Reset after wait
                now = time.time()
                self.tokens_used = 0
                self.token_reset_time = now + 60

            # Reserve tokens for this request
            self.tokens_used += estimated_tokens

    def request(self, estimated_tokens: int, repo_id: Optional[str] = None):
        """Get rate limit context for a request.

        Args:
            estimated_tokens: Estimated tokens for the request
            repo_id: Optional repository identifier

        Returns:
            RateLimitContext instance
        """
        return RateLimitContext(self, estimated_tokens, repo_id)


class LLMClient:
    """LLM client with OpenAI-compatible API support.

    Features:
    - Rate limiting (requests/min and tokens/min)
    - Concurrency control
    - Robust JSON parsing with 5 fallback strategies
    - Response caching (added via cache parameter)
    - Retry logic with exponential backoff
    """

    def __init__(
        self,
        endpoint: str,
        model: str,
        api_key: Optional[str] = None,
        temperature: float = 0.2,
        max_tokens: int = 8000,
        timeout: int = 60,
        max_retries: int = 3,
        retry_delay: int = 2,
        exponential_backoff: bool = True,
        max_concurrent_global: int = 5,
        requests_per_minute: int = 60,
        max_tokens_per_minute: int = 100000,
        cache: Optional["IntelligentCache"] = None,
        repo_path: Optional[Path] = None,
    ):
        """Initialize LLM client.

        Args:
            endpoint: OpenAI-compatible API endpoint
            model: Model name to use
            api_key: Optional API key
            temperature: Sampling temperature (0.0-2.0)
            max_tokens: Maximum tokens per request
            timeout: Request timeout in seconds
            max_retries: Maximum retry attempts
            retry_delay: Initial retry delay in seconds
            exponential_backoff: Use exponential backoff for retries
            max_concurrent_global: Maximum concurrent requests
            requests_per_minute: Rate limit for requests
            max_tokens_per_minute: Rate limit for tokens
            cache: Optional cache instance for response caching
            repo_path: Optional repository path for commit SHA retrieval
        """
        self.endpoint = endpoint
        self.model = model
        self.temperature = temperature
        self.max_tokens = max_tokens
        self.timeout = timeout
        self.max_retries = max_retries
        self.retry_delay = retry_delay
        self.exponential_backoff = exponential_backoff
        self.cache = cache
        self.repo_path = repo_path

        # Initialize OpenAI client
        self.client = AsyncOpenAI(
            base_url=endpoint,
            api_key=api_key or "dummy-key",  # Some endpoints don't need a key
            timeout=timeout,
        )

        # Initialize rate limiter
        self.rate_limiter = RateLimiter(
            max_concurrent=max_concurrent_global,
            requests_per_minute=requests_per_minute,
            max_tokens_per_minute=max_tokens_per_minute,
        )

        # Metrics
        self.total_requests = 0
        self.total_tokens = 0
        self.failed_requests = 0

    async def analyze_code(
        self,
        system_prompt: str,
        code: str,
        repo_id: Optional[str] = None,
        commit_sha: Optional[str] = None,
    ) -> LLMResponse:
        """Analyze code with LLM.

        Args:
            system_prompt: System prompt describing the task
            code: Code to analyze
            repo_id: Optional repository identifier
            commit_sha: Optional commit SHA (auto-detected if not provided)

        Returns:
            LLMResponse with content and metadata

        Raises:
            Exception: If all retries fail
        """
        # Get commit SHA if not provided
        if commit_sha is None:
            commit_sha = get_current_commit_sha(self.repo_path)

        # Check cache if available
        if self.cache:
            cached = self.cache.get(
                prompt=system_prompt,
                code=code,
                model=self.model,
                temperature=self.temperature,
                commit_sha=commit_sha,
            )
            if cached:
                logger.debug("Cache hit for analyze_code")
                return LLMResponse(
                    content=cached["content"],
                    tokens_used=cached["tokens_used"],
                    latency_ms=cached["latency_ms"],
                    model=cached["model"],
                )

        # Estimate tokens (rough: 4 chars per token)
        estimated_tokens = (len(system_prompt) + len(code) + self.max_tokens) // 4

        # Retry logic
        last_exception = None
        for attempt in range(self.max_retries):
            try:
                async with self.rate_limiter.request(estimated_tokens, repo_id) as ctx:
                    # Make request
                    start_time = time.time()

                    response = await self.client.chat.completions.create(
                        model=self.model,
                        messages=[
                            {"role": "system", "content": system_prompt},
                            {"role": "user", "content": code},
                        ],
                        temperature=self.temperature,
                        max_tokens=self.max_tokens,
                    )

                    latency_ms = (time.time() - start_time) * 1000

                    # Extract response
                    content = response.choices[0].message.content
                    tokens_used = response.usage.total_tokens

                    # Update actual tokens
                    ctx.set_actual_tokens(tokens_used)

                    # Update metrics
                    self.total_requests += 1
                    self.total_tokens += tokens_used

                    # Create response object
                    llm_response = LLMResponse(
                        content=content,
                        tokens_used=tokens_used,
                        latency_ms=latency_ms,
                        model=response.model,
                    )

                    # Cache response if available
                    if self.cache:
                        self.cache.set(
                            prompt=system_prompt,
                            code=code,
                            model=self.model,
                            temperature=self.temperature,
                            commit_sha=commit_sha,
                            response={
                                "content": content,
                                "tokens_used": tokens_used,
                                "latency_ms": latency_ms,
                                "model": response.model,
                            },
                            tokens_used=tokens_used,
                            latency_ms=latency_ms,
                        )

                    return llm_response

            except Exception as e:
                last_exception = e
                self.failed_requests += 1

                if attempt < self.max_retries - 1:
                    # Calculate backoff delay
                    if self.exponential_backoff:
                        delay = self.retry_delay * (2**attempt)
                    else:
                        delay = self.retry_delay

                    logger.warning(
                        f"LLM request failed (attempt {attempt + 1}/{self.max_retries}): {e}. "
                        f"Retrying in {delay}s..."
                    )
                    await asyncio.sleep(delay)
                else:
                    logger.error(f"LLM request failed after {self.max_retries} attempts: {e}")

        # All retries failed
        raise last_exception

    async def analyze_code_json(
        self,
        system_prompt: str,
        code: str,
        schema: Optional[Type[BaseModel]] = None,
        repo_id: Optional[str] = None,
        commit_sha: Optional[str] = None,
    ) -> Dict[str, Any]:
        """Analyze code and parse JSON response with fallback strategies.

        Implements 5 fallback strategies:
        1. Extract from markdown code fences
        2. Direct JSON parse
        3. Fix common errors (trailing commas, single quotes)
        4. Recover truncated JSON (add missing brackets)
        5. Fuzzy inference using schema (if provided)

        Args:
            system_prompt: System prompt (should request JSON output)
            code: Code to analyze
            schema: Optional Pydantic model for validation and fuzzy inference
            repo_id: Optional repository identifier
            commit_sha: Optional commit SHA (auto-detected if not provided)

        Returns:
            Parsed JSON dict

        Raises:
            ValueError: If all parsing strategies fail
        """
        # Retry up to 3 times with increasingly strict prompts
        for attempt in range(3):
            response = await self.analyze_code(
                system_prompt=system_prompt,
                code=code,
                repo_id=repo_id,
                commit_sha=commit_sha,
            )
            content = response.content

            # Strategy 1: Extract from markdown fences
            if "```json" in content or "```" in content:
                match = re.search(r"```(?:json)?\n(.*?)\n```", content, re.DOTALL)
                if match:
                    content = match.group(1).strip()

            # Strategy 2: Direct parse
            try:
                result = json.loads(content)
                if schema:
                    # Validate with Pydantic
                    validated = schema(**result)
                    return validated.model_dump()
                return result
            except json.JSONDecodeError:
                pass

            # Strategy 3: Fix common errors
            try:
                # Remove trailing commas before } or ]
                cleaned = re.sub(r",(\s*[}\]])", r"\1", content)
                # Replace single quotes with double quotes (naive)
                cleaned = cleaned.replace("'", '"')
                result = json.loads(cleaned)
                if schema:
                    validated = schema(**result)
                    return validated.model_dump()
                return result
            except (json.JSONDecodeError, Exception):
                pass

            # Strategy 4: Recover truncated JSON
            try:
                # Count braces
                open_braces = content.count("{")
                close_braces = content.count("}")
                open_brackets = content.count("[")
                close_brackets = content.count("]")

                recovered = content
                if open_braces > close_braces:
                    recovered += "}" * (open_braces - close_braces)
                if open_brackets > close_brackets:
                    recovered += "]" * (open_brackets - close_brackets)

                result = json.loads(recovered)
                if schema:
                    validated = schema(**result)
                    return validated.model_dump()
                return result
            except (json.JSONDecodeError, Exception):
                pass

            # Strategy 5: Fuzzy inference (last resort, attempt 2 only)
            if attempt == 2 and schema:
                try:
                    result = self._fuzzy_inference(content, schema)
                    if result:
                        return result
                except Exception as e:
                    logger.debug(f"Fuzzy inference failed: {e}")

            # Retry with stricter prompt
            if attempt < 2:
                system_prompt += "\n\nIMPORTANT: Return ONLY valid, well-formed JSON. No explanations, no markdown fences."

        raise ValueError(f"Failed to parse JSON after 3 attempts. Last content: {content[:200]}...")

    def _fuzzy_inference(self, content: str, schema: Type[BaseModel]) -> Optional[Dict[str, Any]]:
        """Attempt to extract data from malformed response using schema.

        Uses regex to extract values for expected fields.

        Args:
            content: Malformed response content
            schema: Pydantic model schema

        Returns:
            Extracted dict or None if extraction fails
        """
        # Get schema fields
        fields = schema.model_fields

        result = {}
        for field_name, field_info in fields.items():
            # Try to extract field value using various patterns
            patterns = [
                # "field_name": "value"
                rf'"{field_name}"\s*:\s*"([^"]*)"',
                # "field_name": value (number/boolean)
                rf'"{field_name}"\s*:\s*([^,\}}\]]+)',
                # field_name: "value"
                rf"{field_name}\s*:\s*\"([^\"]*)\"",
                # Natural language: "field_name is value"
                rf'{field_name}\s+is\s+"([^"]*)"',
                # Natural language: field_name is value (number)
                rf"{field_name}\s+is\s+(\d+)",
            ]

            for pattern in patterns:
                match = re.search(pattern, content, re.IGNORECASE)
                if match:
                    value = match.group(1).strip()
                    # Try to convert to appropriate type
                    if field_info.annotation == int:
                        try:
                            result[field_name] = int(value)
                        except ValueError:
                            pass
                    elif field_info.annotation == float:
                        try:
                            result[field_name] = float(value)
                        except ValueError:
                            pass
                    elif field_info.annotation == bool:
                        result[field_name] = value.lower() in ("true", "1", "yes")
                    else:
                        result[field_name] = value
                    break

        # Validate extracted data
        if result:
            try:
                validated = schema(**result)
                return validated.model_dump()
            except Exception:
                pass

        return None

    def get_metrics(self) -> Dict[str, Any]:
        """Get client metrics.

        Returns:
            Dict with metrics
        """
        success_rate = (
            (self.total_requests - self.failed_requests) / self.total_requests
            if self.total_requests > 0
            else 0.0
        )

        avg_tokens = self.total_tokens / self.total_requests if self.total_requests > 0 else 0

        return {
            "total_requests": self.total_requests,
            "failed_requests": self.failed_requests,
            "total_tokens": self.total_tokens,
            "success_rate": success_rate,
            "avg_tokens_per_request": avg_tokens,
        }

    async def close(self):
        """Close the client and release resources."""
        await self.client.close()
