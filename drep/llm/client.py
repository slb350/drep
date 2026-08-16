"""LLM client for OpenAI-compatible APIs.

Production-ready client with dual-backend support (open-agent-sdk preferred,
raw httpx fallback), intelligent caching, circuit-breaker-guarded transports,
retries, and robust JSON parsing. Rate limiting lives in ``drep.llm.rate_limiter``
(``RateLimiter`` is re-exported here for backward compatibility).

::

    client = LLMClient(
        endpoint="http://localhost:1234/v1",
        model="local-model",
        max_concurrent_global=5,
        requests_per_minute=60,
        max_tokens_per_minute=MAX_TOKENS_PER_MINUTE,
    )

    response = await client.analyze_code(
        system_prompt="Review this code for bugs",
        code="def foo(): pass",
        repo_id="my-repo",
    )

    result = await client.analyze_code_json(
        system_prompt="Return JSON with findings",
        code="def foo(): pass",
        schema=MyPydanticModel,
    )
"""

import asyncio  # For async/await and concurrency primitives (Semaphore, Lock)
import logging  # For structured logging throughout the module
import time  # For measuring request latency
from collections.abc import Callable  # For transport guard type hint
from dataclasses import dataclass  # For simple data classes (LLMResponse)
from pathlib import Path  # For cross-platform file path handling
from typing import TYPE_CHECKING, Any  # Type hints for better IDE support and clarity

import httpx  # Modern async HTTP client (fallback when open-agent-sdk unavailable)
from pydantic import BaseModel  # For JSON schema validation and type safety

from drep.constants import (
    DEFAULT_MAX_TOKENS_PER_REQUEST,
    MAX_ESTIMATED_TOKENS,
    MAX_TOKENS_PER_MINUTE,
)
from drep.llm.circuit_breaker import (  # Prevents cascade failures
    CircuitBreaker,
    CircuitBreakerOpenError,
)
from drep.llm.git_utils import get_current_commit_sha
from drep.llm.json_parsing import extract_json
from drep.llm.metrics import LLMMetrics  # Tracks usage statistics for cost monitoring
from drep.llm.rate_limiter import RateLimiter
from drep.logging_utils import sanitize_secrets

if TYPE_CHECKING:
    from drep.llm.cache import IntelligentCache

logger = logging.getLogger(__name__)

# Sentinel value for distinguishing "not provided" from "explicitly None"
_UNSET = object()


@dataclass
class LLMResponse:
    """Structured response from an LLM request with metadata.

    This dataclass wraps the LLM's response content along with usage metrics
    that are useful for:
    - Cost tracking (tokens_used)
    - Performance monitoring (latency_ms)
    - Model version tracking (model)
    - Caching (all fields stored in cache)

    Attributes:
        content: The actual text response from the LLM (e.g., analysis results,
                 generated docstrings, JSON findings). This is the primary output.
        tokens_used: Total tokens consumed by this request (prompt + completion).
                     Used for cost calculation and rate limiting.
        latency_ms: Request latency in milliseconds from request start to response.
                    Used for performance monitoring and SLA tracking.
        model: The actual model name that served the request. May differ from
               requested model if the LLM server does model aliasing.
    """

    content: str  # The LLM's response text
    tokens_used: int  # Total tokens (prompt + completion)
    latency_ms: float  # Request duration in milliseconds
    model: str  # Actual model name used


class LLMClient:
    r"""Production-ready LLM client for OpenAI-compatible APIs with advanced features.

    This is the main entry point for all LLM operations in drep. It provides a robust,
    production-ready interface that handles the complexities of LLM integration:

    Core Features:
    --------------
    1. **Dual Backend Support**:
       - Prefers open-agent-sdk when available (better performance, native OpenAI SDK)
       - Falls back to raw HTTP via httpx (universal compatibility)
       - Transparent switching - same interface for both

    2. **Multi-level Rate Limiting** (see RateLimiter class):
       - Global concurrency limits (don't overwhelm LLM server)
       - Per-repo concurrency limits (fair resource sharing)
       - Requests-per-minute throttling (respect API limits)
       - Tokens-per-minute throttling (cost control)

    3. **Intelligent Caching**:
       - Content-based keys: (prompt + code + model + temperature + commit_sha)
       - Automatic invalidation on code changes (new commit)
       - Typical cache hit rate: 80%+ on incremental scans
       - Dramatic cost and latency reduction

    4. **Robust JSON Parsing** (5-level fallback strategy):
       - Level 1: Extract from markdown code fences (\`\`\`json)
       - Level 2: Direct JSON parse
       - Level 3: Fix common errors (trailing commas, single quotes)
       - Level 4: Recover truncated JSON (add missing brackets)
       - Level 5: Fuzzy inference from schema (last resort)

    5. **Reliability Features**:
       - Exponential backoff retries (configurable attempts and delays)
       - Circuit breaker pattern (optional, prevents cascade failures)
       - Comprehensive metrics tracking (cost, latency, success rates)
       - Graceful degradation

    Architecture:
    -------------
    The client uses dependency injection for caching and follows the async/await
    pattern throughout. Rate limiting is enforced via async context managers
    that hold semaphores for the entire request duration.

    Typical Request Flow:
    ---------------------
    1. Check cache (if enabled) → return immediately if hit
    2. Acquire rate limit context (may sleep if limits exceeded)
    3. Make LLM API request (with retries on failure)
    4. Update metrics (tokens, latency, success/failure)
    5. Store in cache (if enabled)
    6. Release rate limit context (reconcile actual tokens)
    7. Return response

    Usage Examples:
    ---------------

    ::

        # Initialize with local LLM (LM Studio, Ollama, etc.)
        client = LLMClient(
            endpoint="http://localhost:1234/v1",
            model="local-model",
            api_key="not-needed",  # Many local LLMs don't need keys
            max_concurrent_global=5,
            requests_per_minute=60,
            max_tokens_per_minute=MAX_TOKENS_PER_MINUTE,
        )

        # Simple text analysis
        response = await client.analyze_code(
            system_prompt="Review this Python code for bugs",
            code="def divide(a, b): return a / b",
            repo_id="my-org/my-repo",
        )
        print(f"Analysis: {response.content}")
        print(f"Tokens used: {response.tokens_used}")

        # JSON analysis with schema validation
        from pydantic import BaseModel
        class BugReport(BaseModel):
            bugs: list[str]
            severity: str

        result = await client.analyze_code_json(
            system_prompt="Return JSON: {bugs: [...], severity: 'high'|'medium'|'low'}",
            code="def divide(a, b): return a / b",
            schema=BugReport,  # Validates and provides fallback parsing
        )
        print(f"Found {len(result['bugs'])} bugs")

        # Don't forget to close when done
        await client.close()
    """

    def __init__(
        self,
        endpoint: str,
        model: str,
        api_key: str | None = None,
        temperature: float = 0.2,
        max_tokens: int = DEFAULT_MAX_TOKENS_PER_REQUEST,
        timeout: int = 60,
        max_retries: int = 3,
        retry_delay: int = 2,
        exponential_backoff: bool = True,
        max_concurrent_global: int = 5,
        max_concurrent_per_repo: int | None = 3,
        requests_per_minute: int = 60,
        max_tokens_per_minute: int = MAX_TOKENS_PER_MINUTE,
        cache: "IntelligentCache | None" = None,
        repo_path: Path | None = None,
        rate_limiter: RateLimiter | None = None,
        enable_circuit_breaker: bool = True,
        circuit_breaker: CircuitBreaker | None = _UNSET,  # type: ignore
        circuit_breaker_threshold: int = 5,
        circuit_breaker_timeout: int = 60,
        provider: str = "openai-compatible",
        bedrock_region: str | None = None,
        bedrock_model: str | None = None,
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
                (ignored if rate_limiter provided)

            max_concurrent_per_repo: Maximum concurrent requests per repository
                (ignored if rate_limiter provided)

            requests_per_minute: Rate limit for requests
                (ignored if rate_limiter provided)

            max_tokens_per_minute: Rate limit for tokens
                (ignored if rate_limiter provided)

            cache: Optional cache instance for response caching
            repo_path: Optional repository path for commit SHA retrieval
            rate_limiter: Optional RateLimiter instance (creates default if None)

            enable_circuit_breaker: Enable circuit breaker pattern
                (ignored if circuit_breaker provided)

            circuit_breaker: Optional CircuitBreaker instance
                (None to disable, creates default if not provided)

            circuit_breaker_threshold: Failures before opening circuit
                (ignored if circuit_breaker provided)

            circuit_breaker_timeout: Recovery timeout in seconds
                (ignored if circuit_breaker provided)
        """
        # Store configuration parameters
        # Bedrock doesn't need endpoint, so handle None gracefully
        self.endpoint = endpoint.rstrip("/") if endpoint else None
        self.model = model  # Model name (e.g., "gpt-4", "llama-2-70b", etc.)
        self.temperature = temperature  # Sampling temperature: lower = more deterministic
        self.max_tokens = max_tokens  # Maximum completion tokens per request
        self.timeout = timeout  # HTTP timeout in seconds
        self.max_retries = max_retries  # Number of retry attempts on failure
        self.retry_delay = retry_delay  # Initial delay between retries (seconds)
        self.exponential_backoff = exponential_backoff  # Whether to use exponential backoff
        self.cache = cache  # Optional IntelligentCache instance for response caching
        self.repo_path = repo_path  # Optional repo path for commit SHA retrieval
        self._provider = provider  # LLM provider: openai-compatible or bedrock

        # === PROVIDER SELECTION: Bedrock, open-agent-sdk, or HTTP ===
        # Check if Bedrock provider is requested
        if provider == "bedrock":
            # Lazy: boto3 is heavy and only needed for the Bedrock provider
            from drep.llm.providers.bedrock_client import BedrockClient  # noqa: PLC0415

            if not bedrock_region or not bedrock_model:
                raise ValueError(
                    "Bedrock provider requires bedrock_region and bedrock_model parameters"
                )

            self.bedrock_client = BedrockClient(
                region=bedrock_region,
                model=bedrock_model,
            )

            # CRITICAL: Preserve Bedrock model in self.model for cache keys and metadata
            # Without this, cache lookups use model=None and different Bedrock models
            # can serve stale cached results from each other
            self.model = bedrock_model

            logger.info(
                f"LLM backend: AWS Bedrock (region={bedrock_region}, model={bedrock_model})"
            )

            self._init_runtime(
                rate_limiter,
                max_concurrent_global,
                requests_per_minute,
                max_tokens_per_minute,
                max_concurrent_per_repo,
                circuit_breaker,
                enable_circuit_breaker,
                circuit_breaker_threshold,
                circuit_breaker_timeout,
            )
            return  # Skip open-agent-sdk/HTTP initialization

        # === BACKEND SELECTION: open-agent-sdk vs HTTP ===
        # We support two backends with identical interfaces:
        # 1. open-agent-sdk: Preferred, better performance, more features
        # 2. HTTP (httpx): Fallback, universal compatibility

        self._using_open_agent = False  # Flag to track which backend is active
        # AsyncOpenAI instance or HTTP compat shim; both expose .chat.completions.create
        self.client: Any = None

        # Try to initialize open-agent-sdk (preferred backend)
        try:
            # Lazy: keep import cost off the plain-HTTP fallback path
            from open_agent.types import AgentOptions  # type: ignore  # noqa: PLC0415
            from open_agent.utils import create_client  # type: ignore  # noqa: PLC0415

            # Configure open-agent-sdk with our settings
            options = AgentOptions(
                system_prompt="",  # System prompt is provided per-request, not here
                model=self.model,
                base_url=self.endpoint,
                timeout=self.timeout,
                api_key=api_key or "not-needed",  # Local LLMs often don't need keys
            )
            self.client = create_client(options)  # Returns AsyncOpenAI-compatible instance
            self._using_open_agent = True
            logger.info("LLM backend: open-agent-sdk (OpenAI-compatible)")

        except ImportError:
            # open-agent-sdk not installed - this is fine, we'll use HTTP fallback
            logger.info("LLM backend: HTTP (OpenAI-compatible), open-agent-sdk not installed")
        except Exception as e:
            # open-agent-sdk is installed but failed to initialize (config error, etc.)
            # Fall back to HTTP to ensure we can still operate
            logger.warning(f"open-agent-sdk initialization failed, falling back to HTTP: {e}")

        # Initialize HTTP client (used when open-agent-sdk unavailable)
        self.http = None
        if not self._using_open_agent:
            # Build HTTP headers for OpenAI-compatible API
            headers = {}
            if api_key:
                # Most LLM APIs use Bearer token authentication
                headers["Authorization"] = f"Bearer {api_key}"
            headers["Content-Type"] = "application/json"

            # Create async HTTP client with base URL and headers
            # This will be used to make POST requests to /chat/completions
            self.http = httpx.AsyncClient(
                base_url=self.endpoint or "", headers=headers, timeout=timeout
            )

        # === COMPATIBILITY SHIM FOR HTTP BACKEND ===
        # The following classes create an OpenAI SDK-like interface for our HTTP client.
        # This allows us to use the same code path regardless of backend:
        #     response = await self.client.chat.completions.create(...)
        #
        # Works for both:
        # - open-agent-sdk: Already provides this interface (AsyncOpenAI)
        # - HTTP backend: We create this interface via nested compat classes
        #
        # This also makes testing easier - tests can mock client.chat.completions.create
        # uniformly without caring which backend is active.
        #
        # The shim wraps raw HTTP responses in OpenAI-like objects:
        #     HTTP response → _CompatResponse → response.choices[0].message.content
        client_self = self

        class _CompatMessage:
            def __init__(self, content: str):
                self.content = content

        class _CompatChoice:
            def __init__(self, content: str):
                self.message = _CompatMessage(content)

        class _CompatUsage:
            def __init__(self, usage: dict[str, Any]):
                prompt = usage.get("prompt_tokens") or usage.get("input_tokens") or 0
                completion = usage.get("completion_tokens") or usage.get("output_tokens") or 0
                self.prompt_tokens = prompt
                self.completion_tokens = completion
                self.total_tokens = usage.get("total_tokens") or (prompt + completion)

        class _CompatResponse:
            def __init__(self, data: dict[str, Any]):
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

        self._init_runtime(
            rate_limiter,
            max_concurrent_global,
            requests_per_minute,
            max_tokens_per_minute,
            max_concurrent_per_repo,
            circuit_breaker,
            enable_circuit_breaker,
            circuit_breaker_threshold,
            circuit_breaker_timeout,
        )

    async def _guarded_transport(self, func: Callable, **kwargs: Any) -> Any:
        """Invoke a provider transport call, guarded by the circuit breaker when enabled."""
        if self.circuit_breaker is None:
            return await func(**kwargs)
        return await self.circuit_breaker.call(func, **kwargs)

    def _init_runtime(
        self,
        rate_limiter: "RateLimiter | None",
        max_concurrent_global: int,
        requests_per_minute: int,
        max_tokens_per_minute: int,
        max_concurrent_per_repo: int | None,
        circuit_breaker: Any,
        enable_circuit_breaker: bool,
        circuit_breaker_threshold: int,
        circuit_breaker_timeout: int,
    ) -> None:
        """Set up the backend-independent runtime: limiter, metrics, breaker.

        Called once from each provider branch of __init__. These three pieces
        sit in front of whichever transport was selected, so they are identical
        for Bedrock and for the OpenAI-compatible/HTTP path.
        """
        # Rate limiter: injected (dependency injection) or a default
        if rate_limiter is not None:
            self.rate_limiter = rate_limiter
        else:
            self.rate_limiter = RateLimiter(
                max_concurrent=max_concurrent_global,
                requests_per_minute=requests_per_minute,
                max_tokens_per_minute=max_tokens_per_minute,
                max_concurrent_per_repo=max_concurrent_per_repo,
            )

        # Metrics tracking
        self.metrics = LLMMetrics()

        # Circuit breaker: an explicit value (instance or None) always wins over
        # the enable_circuit_breaker flag, which is the backward-compatible path.
        if circuit_breaker is not _UNSET:
            self.circuit_breaker = circuit_breaker
        elif enable_circuit_breaker:
            self.circuit_breaker = CircuitBreaker(
                failure_threshold=circuit_breaker_threshold,
                recovery_timeout=circuit_breaker_timeout,
            )
        else:
            self.circuit_breaker = None

    async def analyze_code(
        self,
        system_prompt: str,
        code: str,
        repo_id: str | None = None,
        commit_sha: str | None = None,
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
        # Get commit SHA if not provided. This forks `git rev-parse`, so run it
        # off the event loop: a synchronous fork/exec here stalls every other
        # in-flight request, and analyze_code_json can reach this up to
        # max_retries times.
        if commit_sha is None:
            commit_sha = await asyncio.to_thread(get_current_commit_sha, self.repo_path)

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
        estimated_tokens = max(1, min(estimated_tokens, MAX_ESTIMATED_TOKENS))

        # Retry logic
        last_exception = None
        for attempt in range(self.max_retries):
            try:
                async with self.rate_limiter.request(estimated_tokens, repo_id) as ctx:
                    # Make request
                    start_time = time.time()

                    # Use Bedrock provider if configured.
                    # Bedrock returns a dict; the OpenAI-compatible path returns an
                    # SDK-like object, so the handling below branches on provider.
                    # Both transports run through the circuit breaker when enabled.
                    response: Any
                    if self._provider == "bedrock":
                        response = await self._guarded_transport(
                            self.bedrock_client.chat_completion,
                            messages=[
                                {"role": "system", "content": system_prompt},
                                {"role": "user", "content": code},
                            ],
                            temperature=self.temperature,
                            max_tokens=self.max_tokens,
                        )
                    else:
                        # Use OpenAI-compatible provider (open-agent-sdk or HTTP)
                        response = await self._guarded_transport(
                            self.client.chat.completions.create,
                            model=self.model,
                            messages=[
                                {"role": "system", "content": system_prompt},
                                {"role": "user", "content": code},
                            ],
                            temperature=self.temperature,
                            max_tokens=self.max_tokens,
                        )

                    latency_ms = (time.time() - start_time) * 1000

                    # Extract response (handle both dict and object formats)
                    if self._provider == "bedrock":
                        # Bedrock returns dict
                        content = response["choices"][0]["message"]["content"]
                        tokens_used = response["usage"]["total_tokens"]
                        prompt_tokens = response["usage"]["prompt_tokens"]
                        completion_tokens = response["usage"]["completion_tokens"]
                    else:
                        # OpenAI-compatible returns object
                        content = response.choices[0].message.content
                        tokens_used = response.usage.total_tokens
                        prompt_tokens = response.usage.prompt_tokens
                        completion_tokens = response.usage.completion_tokens

                    # Update actual tokens
                    ctx.set_actual_tokens(tokens_used)

                    # Record metrics
                    self.metrics.record_request(
                        analyzer=analyzer,
                        success=True,
                        cached=False,
                        tokens_prompt=prompt_tokens,
                        tokens_completion=completion_tokens,
                        latency_ms=latency_ms,
                    )

                    # Create response object
                    llm_response = LLMResponse(
                        content=content,
                        tokens_used=tokens_used,
                        latency_ms=latency_ms,
                        model=self.model if self._provider == "bedrock" else response.model,
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
                                "model": (
                                    self.model if self._provider == "bedrock" else response.model
                                ),
                            },
                            tokens_used=tokens_used,
                            latency_ms=latency_ms,
                        )

                    return llm_response

            except CircuitBreakerOpenError:  # noqa: PERF203 - fail fast, not a retry candidate
                # Fail fast: an open circuit must not be retried or counted
                # as a transport failure
                raise
            except Exception as e:
                last_exception = e

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

                    error_msg = sanitize_secrets(str(e))

                    logger.warning(
                        f"LLM request failed (attempt {attempt + 1}/"
                        f"{self.max_retries}): {error_msg}. Retrying in {delay}s..."
                    )
                    await asyncio.sleep(delay)
                else:
                    error_msg = sanitize_secrets(str(e))
                    logger.error(
                        f"LLM request failed after {self.max_retries} attempts: {error_msg}"
                    )

        # All retries failed
        if last_exception is not None:
            raise last_exception
        raise RuntimeError("LLM request failed but no exception was captured")

    async def analyze_code_json(
        self,
        system_prompt: str,
        code: str,
        schema: type[BaseModel] | None = None,
        repo_id: str | None = None,
        commit_sha: str | None = None,
        analyzer: str = "unknown",
    ) -> dict[str, Any]:
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

            result = extract_json(content, schema, allow_fuzzy=(attempt == 2))
            if result is not None:
                return result

            # Retry with stricter prompt
            if attempt < 2:
                system_prompt += (
                    "\n\nIMPORTANT: Return ONLY valid, well-formed JSON. "
                    "No explanations, no markdown fences."
                )

        raise ValueError(f"Failed to parse JSON after 3 attempts. Last content: {content[:200]}...")

    def get_metrics(self) -> dict[str, Any]:
        """Get current metrics as dictionary.

        Returns:
            Dict with metrics from metrics object
        """
        return self.metrics.to_dict()

    def get_llm_metrics(self) -> LLMMetrics:
        """Get LLMMetrics object with detailed statistics.

        Returns:
            LLMMetrics object with comprehensive usage statistics
        """
        return self.metrics

    async def close(self):
        """Close the client and release resources."""
        # Close Bedrock client if using Bedrock provider
        if self._provider == "bedrock" and hasattr(self, "bedrock_client"):
            await self.bedrock_client.close()
            return

        # Prefer closing compat client to satisfy tests that patch client.close
        if hasattr(self, "client") and hasattr(self.client, "close"):
            try:
                await self.client.close()
                return
            except Exception:
                pass
        if hasattr(self, "http") and self.http:
            await self.http.aclose()
