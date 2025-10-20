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

import httpx
from pydantic import BaseModel

from drep.llm.circuit_breaker import CircuitBreaker
from drep.llm.metrics import LLMMetrics

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
        self.repo_semaphore: Optional[asyncio.Semaphore] = None

    async def __aenter__(self):
        """Acquire semaphore and check rate limits."""
        # Acquire global semaphore (held until __aexit__)
        await self.rate_limiter.semaphore.acquire()

        # Acquire per-repo semaphore if repo_id specified
        if self.repo_id is not None:
            self.repo_semaphore = await self.rate_limiter._get_repo_semaphore(self.repo_id)
            await self.repo_semaphore.acquire()

        # Check request rate limit
        await self.rate_limiter._check_request_rate_limit()

        # Check token rate limit
        await self.rate_limiter._check_token_rate_limit(self.estimated_tokens)

        return self

    async def __aexit__(self, exc_type, exc_val, exc_tb):
        """Update token usage and release semaphores."""
        async with self.rate_limiter.lock:
            # Update actual token usage if set
            if self.actual_tokens is not None:
                # Adjust token count: remove estimate, add actual
                # Clamp to 0 to prevent negatives (e.g., if bucket reset while request was running)
                self.rate_limiter.tokens_used = max(
                    0, self.rate_limiter.tokens_used - self.estimated_tokens
                )
                self.rate_limiter.tokens_used += self.actual_tokens
            else:
                # Request failed before set_actual_tokens() was called
                # Roll back the estimated token reservation
                # Clamp to 0 to prevent negatives
                self.rate_limiter.tokens_used = max(
                    0, self.rate_limiter.tokens_used - self.estimated_tokens
                )
                logger.debug(
                    f"Rolling back {self.estimated_tokens} token reservation "
                    f"(request failed without completion)"
                )

        # Release per-repo semaphore if acquired
        if self.repo_semaphore is not None:
            self.repo_semaphore.release()

        # Release global semaphore
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
    1. Maximum concurrent requests globally (semaphore)
    2. Maximum concurrent requests per repository (per-repo semaphores)
    3. Requests per minute limit
    4. Tokens per minute limit
    """

    def __init__(
        self,
        max_concurrent: int,
        requests_per_minute: int,
        max_tokens_per_minute: int,
        max_concurrent_per_repo: Optional[int] = None,
    ):
        """Initialize rate limiter.

        Args:
            max_concurrent: Maximum concurrent requests globally
            requests_per_minute: Request rate limit
            max_tokens_per_minute: Token rate limit
            max_concurrent_per_repo: Maximum concurrent requests per repository
        """
        self.max_concurrent = max_concurrent
        self.requests_per_minute = requests_per_minute
        self.max_tokens_per_minute = max_tokens_per_minute
        self.max_concurrent_per_repo = max_concurrent_per_repo

        # Concurrency control
        self.semaphore = asyncio.Semaphore(max_concurrent)

        # Per-repository concurrency control
        self.repo_semaphores: Dict[str, asyncio.Semaphore] = {}
        self.repo_last_used: Dict[str, float] = {}  # Track last use for cleanup
        self.repo_semaphore_ttl = 600  # Evict idle semaphores after 10 minutes

        # Request rate limiting
        self.lock = asyncio.Lock()
        self.request_times: list[float] = []

        # Token rate limiting
        self.tokens_used = 0
        self.token_reset_time = time.time() + 60

    async def _get_repo_semaphore(self, repo_id: str) -> asyncio.Semaphore:
        """Get or create semaphore for a repository.

        Also performs cleanup of idle semaphores to prevent unbounded growth.

        Args:
            repo_id: Repository identifier

        Returns:
            Semaphore for the repository
        """
        async with self.lock:
            now = time.time()

            # Cleanup: remove idle semaphores that haven't been used recently
            idle_repos = [
                rid
                for rid, last_used in self.repo_last_used.items()
                if now - last_used > self.repo_semaphore_ttl
            ]
            for rid in idle_repos:
                # Only evict if semaphore is not currently held
                # (Check if all permits available = not in use)
                sem = self.repo_semaphores.get(rid)
                if sem is not None and sem._value == (
                    self.max_concurrent_per_repo or self.max_concurrent
                ):
                    del self.repo_semaphores[rid]
                    del self.repo_last_used[rid]
                    logger.debug(f"Evicted idle semaphore for repo {rid}")

            # Get or create semaphore for this repo
            if repo_id not in self.repo_semaphores:
                # Create new semaphore for this repo
                limit = self.max_concurrent_per_repo or self.max_concurrent
                self.repo_semaphores[repo_id] = asyncio.Semaphore(limit)
                logger.debug(f"Created semaphore for repo {repo_id} with limit {limit}")

            # Update last used time
            self.repo_last_used[repo_id] = now

            return self.repo_semaphores[repo_id]

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
                        f"Token rate limit reached "
                        f"({self.tokens_used}/{self.max_tokens_per_minute}), "
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
        max_concurrent_per_repo: Optional[int] = 3,
        requests_per_minute: int = 60,
        max_tokens_per_minute: int = 100000,
        cache: Optional["IntelligentCache"] = None,  # noqa: F821
        repo_path: Optional[Path] = None,
        enable_circuit_breaker: bool = True,
        circuit_breaker_threshold: int = 5,
        circuit_breaker_timeout: int = 60,
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
            max_concurrent_global: Maximum concurrent requests globally
            max_concurrent_per_repo: Maximum concurrent requests per repository
            requests_per_minute: Rate limit for requests
            max_tokens_per_minute: Rate limit for tokens
            cache: Optional cache instance for response caching
            repo_path: Optional repository path for commit SHA retrieval
            enable_circuit_breaker: Enable circuit breaker pattern
            circuit_breaker_threshold: Failures before opening circuit
            circuit_breaker_timeout: Recovery timeout in seconds
        """
        self.endpoint = endpoint.rstrip("/")
        self.model = model
        self.temperature = temperature
        self.max_tokens = max_tokens
        self.timeout = timeout
        self.max_retries = max_retries
        self.retry_delay = retry_delay
        self.exponential_backoff = exponential_backoff
        self.cache = cache
        self.repo_path = repo_path

        # Try to use open-agent-sdk if available (preferred)
        self._using_open_agent = False
        self.client = None
        try:
            from open_agent.types import AgentOptions  # type: ignore
            from open_agent.utils import create_client  # type: ignore

            options = AgentOptions(
                system_prompt="",  # Not used here; prompt is provided per request
                model=self.model,
                base_url=self.endpoint,
                timeout=self.timeout,
                api_key=api_key or "not-needed",
            )
            self.client = create_client(options)  # AsyncOpenAI instance
            self._using_open_agent = True
            logger.info("LLM backend: open-agent-sdk (OpenAI-compatible)")
        except ImportError:
            logger.info("LLM backend: HTTP (OpenAI-compatible), open-agent-sdk not installed")
        except Exception as e:
            logger.warning(f"open-agent-sdk initialization failed, falling back to HTTP: {e}")

        # Initialize HTTP client for OpenAI-compatible endpoints (fallback)
        self.http = None
        if not self._using_open_agent:
            headers = {}
            if api_key:
                headers["Authorization"] = f"Bearer {api_key}"
            headers["Content-Type"] = "application/json"
            self.http = httpx.AsyncClient(base_url=self.endpoint, headers=headers, timeout=timeout)

        # Back-compat shim: expose client.chat.completions.create like OpenAI SDK
        # so unit tests and existing code paths can mock/patch consistently.
        client_self = self

        class _CompatMessage:
            def __init__(self, content: str):
                self.content = content

        class _CompatChoice:
            def __init__(self, content: str):
                self.message = _CompatMessage(content)

        class _CompatUsage:
            def __init__(self, usage: Dict[str, Any]):
                prompt = usage.get("prompt_tokens") or usage.get("input_tokens") or 0
                completion = usage.get("completion_tokens") or usage.get("output_tokens") or 0
                self.prompt_tokens = prompt
                self.completion_tokens = completion
                self.total_tokens = usage.get("total_tokens") or (prompt + completion)

        class _CompatResponse:
            def __init__(self, data: Dict[str, Any]):
                self.model = data.get("model", client_self.model)
                content = (
                    ((data.get("choices") or [{}])[0].get("message") or {}).get("content")
                    or data.get("content")
                    or ""
                )
                self.choices = [_CompatChoice(content)]
                self.usage = _CompatUsage(data.get("usage", {}))

        class _CompatCompletions:
            def __init__(self, parent: "LLMClient"):
                self._parent = parent

            async def create(self, model: str, messages: list, temperature: float, max_tokens: int):
                if not self._parent.http:
                    raise RuntimeError("HTTP client not initialized")
                url = f"{self._parent.endpoint}/chat/completions"
                payload = {
                    "model": model,
                    "messages": messages,
                    "temperature": temperature,
                    "max_tokens": max_tokens,
                }
                resp = await self._parent.http.post(url, json=payload)
                resp.raise_for_status()
                return _CompatResponse(resp.json())

        class _CompatChat:
            def __init__(self, parent: "LLMClient"):
                self.completions = _CompatCompletions(parent)

        class _CompatClient:
            def __init__(self, parent: "LLMClient"):
                self.chat = _CompatChat(parent)

            async def close(self):
                if parent.http:
                    await parent.http.aclose()

        parent = self
        if not self._using_open_agent:
            self.client = _CompatClient(self)

        # Initialize rate limiter
        self.rate_limiter = RateLimiter(
            max_concurrent=max_concurrent_global,
            requests_per_minute=requests_per_minute,
            max_tokens_per_minute=max_tokens_per_minute,
            max_concurrent_per_repo=max_concurrent_per_repo,
        )

        # Metrics tracking
        self.metrics = LLMMetrics()

        # Legacy metrics (for backward compatibility)
        self.total_requests = 0
        self.total_tokens = 0
        self.failed_requests = 0

        # Circuit breaker (optional)
        self.circuit_breaker = None
        if enable_circuit_breaker:
            self.circuit_breaker = CircuitBreaker(
                failure_threshold=circuit_breaker_threshold,
                recovery_timeout=circuit_breaker_timeout,
            )

    async def analyze_code(
        self,
        system_prompt: str,
        code: str,
        repo_id: Optional[str] = None,
        commit_sha: Optional[str] = None,
        analyzer: str = "unknown",
    ) -> LLMResponse:
        """Analyze code with LLM.

        Args:
            system_prompt: System prompt describing the task
            code: Code to analyze
            repo_id: Optional repository identifier
            commit_sha: Optional commit SHA (auto-detected if not provided)
            analyzer: Name of the analyzer making the request

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
                # Record cached request
                self.metrics.record_request(
                    analyzer=analyzer,
                    success=True,
                    cached=True,
                    tokens_prompt=0,
                    tokens_completion=cached["tokens_used"],
                    latency_ms=0,
                )
                return LLMResponse(
                    content=cached["content"],
                    tokens_used=cached["tokens_used"],
                    latency_ms=cached["latency_ms"],
                    model=cached["model"],
                )

        # Estimate tokens (rough: 4 chars per token), clamp to avoid over-reservation
        estimated_tokens = (len(system_prompt) + len(code) + self.max_tokens) // 4
        estimated_tokens = max(1, min(estimated_tokens, 50000))

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

                    # Update legacy metrics
                    self.total_requests += 1
                    self.total_tokens += tokens_used

                    # Record metrics
                    self.metrics.record_request(
                        analyzer=analyzer,
                        success=True,
                        cached=False,
                        tokens_prompt=response.usage.prompt_tokens,
                        tokens_completion=response.usage.completion_tokens,
                        latency_ms=latency_ms,
                    )

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

                # Record failed request
                self.metrics.record_request(
                    analyzer=analyzer,
                    success=False,
                    cached=False,
                    tokens_prompt=0,
                    tokens_completion=0,
                    latency_ms=0,
                )

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
        analyzer: str = "unknown",
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
                analyzer=analyzer,
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
                system_prompt += (
                    "\n\nIMPORTANT: Return ONLY valid, well-formed JSON. "
                    "No explanations, no markdown fences."
                )

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
                    if field_info.annotation is int:
                        try:
                            result[field_name] = int(value)
                        except ValueError:
                            pass
                    elif field_info.annotation is float:
                        try:
                            result[field_name] = float(value)
                        except ValueError:
                            pass
                    elif field_info.annotation is bool:
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
        """Get current metrics (legacy dict format for backward compatibility).

        Returns:
            Dict with metrics including legacy fields
        """
        success_rate = (
            (self.total_requests - self.failed_requests) / self.total_requests
            if self.total_requests > 0
            else 0.0
        )

        avg_tokens = self.total_tokens / self.total_requests if self.total_requests > 0 else 0

        # Return merged dict with both legacy and new metrics
        return {
            # Legacy fields
            "total_requests": self.total_requests,
            "failed_requests": self.failed_requests,
            "total_tokens": self.total_tokens,
            "success_rate": success_rate,
            "avg_tokens_per_request": avg_tokens,
            # New metrics object for advanced usage
            "metrics_object": self.metrics,
        }

    def get_llm_metrics(self) -> LLMMetrics:
        """Get LLMMetrics object with detailed statistics.

        Returns:
            LLMMetrics object with comprehensive usage statistics
        """
        return self.metrics

    async def close(self):
        """Close the client and release resources."""
        # Prefer closing compat client to satisfy tests that patch client.close
        if hasattr(self, "client") and hasattr(self.client, "close"):
            try:
                await self.client.close()
                return
            except Exception:
                pass
        await self.http.aclose()
