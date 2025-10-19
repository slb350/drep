# LLM Integration Implementation Plan

**Version:** 1.0
**Date:** 2025-10-18
**Status:** Planning
**Target:** Phase 7 - Post-MVP Enhancement

---

## Table of Contents

1. [Overview](#overview)
2. [Architecture](#architecture)
3. [Configuration](#configuration)
4. [LLM Client Implementation](#llm-client-implementation)
5. [Code Analysis Pipeline](#code-analysis-pipeline)
6. [Analyzer Components](#analyzer-components)
7. [PR Review Workflow](#pr-review-workflow)
8. [Prompt Engineering](#prompt-engineering)
9. [Issue Generation](#issue-generation)
10. [Testing Strategy](#testing-strategy)
11. [Performance Optimization](#performance-optimization)
12. [Deployment & Operations](#deployment--operations)
13. [Implementation Roadmap](#implementation-roadmap)

---

## Overview

### Vision

Transform drep from a basic typo checker into an **intelligent AI code reviewer** that:

- **Generates documentation automatically** - Creates docstrings, comments, and explanations
- **Reviews code for quality** - Detects bugs, security issues, and anti-patterns
- **Enforces best practices** - Python PEP 8, type hints, error handling
- **Provides actionable feedback** - Not just "this is bad" but "try this instead"
- **Reviews PRs intelligently** - Context-aware suggestions on code changes

### Scope

**In Scope (Phase 7):**
- File-level analysis (no vector DB required)
- Python best practices detection
- Missing/poor documentation detection
- Retire legacy spellcheck/pattern analyzers entirely and rely on LLM-driven typo/pattern detection
- Bug and security vulnerability detection
- PR review with diff analysis
- Intelligent issue creation with LLM suggestions

**Out of Scope (Future):**
- Cross-file dependency analysis (requires vector DB)
- Codebase-wide architectural insights
- Multi-language support beyond Python
- Real-time IDE integration

### Success Metrics

- **Quality**: 80%+ of LLM suggestions are actionable
- **Coverage**: Analyze 100% of functions > 10 lines
- **Performance**: < 2 seconds per function analysis
- **Accuracy**: < 5% false positive rate for bugs
- **Value**: Developers find 50%+ of suggestions useful

---

## Architecture

### High-Level Design

```
┌─────────────────────────────────────────────────────────────┐
│                       CLI / API Layer                        │
│  drep scan owner/repo --llm                                  │
│  drep review <pr-number>                                     │
└────────────────────┬────────────────────────────────────────┘
                     │
┌────────────────────▼────────────────────────────────────────┐
│                  Analysis Coordinator                        │
│  Orchestrates: Pattern → Spellcheck → LLM → Issue Creation  │
└────────────────────┬────────────────────────────────────────┘
                     │
         ┌───────────┐
         │           │
┌────────▼──────────┐
│   Layer 3: LLM    │
│   (Intelligent)   │
│                   │
│ • LLM Client      │
│ • Code Quality    │
│ • Docstring Gen   │
│ • Bug Detection   │
│ • PR Review       │
└────────┬──────────┘
         │
┌────────▼──────────┐
│   LM Studio       │
│   (Remote)        │
│                   │
│ Qwen3-30B-A3B     │
│ 20k context       │
│ 8k max tokens     │
└───────────────────┘
```

### Component Breakdown

#### 1. LLM Client (`drep/llm/client.py`)
- **Purpose**: Abstract OpenAI-compatible API communication
- **Responsibilities**:
  - Connect to LM Studio endpoint
  - Send prompts with code
  - Parse JSON responses
  - Handle retries and errors
  - Track token usage

#### 2. Code Quality Analyzer (`drep/analyzers/code_quality.py`)
- **Purpose**: Analyze Python code for quality issues
- **Responsibilities**:
  - Extract functions from AST
  - Filter functions by complexity/length
  - Send to LLM for analysis
  - Parse and validate responses
  - Generate Finding objects

#### 3. Docstring Generator (`drep/analyzers/docstring_generator.py`)
- **Purpose**: Generate missing docstrings
- **Responsibilities**:
  - Detect functions without docstrings
  - Extract function signature and body
  - Generate Google-style docstrings
  - Validate generated docstrings
  - Return as suggestions

#### 4. PR Review Analyzer (`drep/analyzers/pr_review.py`)
- **Purpose**: Intelligent PR review
- **Responsibilities**:
  - Fetch PR diff from Gitea
  - Extract changed code blocks
  - Analyze changes in context
  - Generate review comments
  - Post comments to PR

#### 5. Analysis Coordinator (`drep/core/analysis_coordinator.py`)
- **Purpose**: Orchestrate LLM-first analysis
- **Responsibilities**:
  - Coordinate AST extraction and LLM analyzers
  - Aggregate findings
  - Deduplicate and prioritize

---

## Configuration

### Config File Structure

```yaml
# config.yaml

gitea:
  url: http://192.168.1.14:3000
  token: ${GITEA_TOKEN}
  repositories:
    - steve/*

llm:
  enabled: true
  endpoint: https://lmstudio.localbrandonfamily.com/v1
  model: qwen/qwen3-30b-a3b-2507
  api_key: ${LM_STUDIO_KEY}  # Optional, may not be needed

  # Model parameters
  temperature: 0.2           # Low for consistent code analysis
  max_tokens: 8000           # Per-file analysis (20k total context)
  timeout: 60                # Request timeout in seconds

  # Retry configuration
  max_retries: 3
  retry_delay: 2             # Seconds between retries
  exponential_backoff: true  # 2s, 4s, 8s delays

  # Concurrency & rate limiting
  max_concurrent_global: 5         # Max parallel requests across all repos
  max_concurrent_per_repo: 3       # Max parallel requests per repo
  requests_per_minute: 60          # Request-based rate limit
  max_tokens_per_minute: 100000    # Token-aware rate limiting

  # Response caching (commit SHA-aware)
  cache:
    enabled: true
    directory: ~/.cache/drep/llm
    ttl_days: 30               # Cache expiry
    max_size_gb: 10.0          # Auto-prune when exceeded
    invalidate_on_commit: true # Invalidate when code changes

  # Telemetry (opt-in only)
  telemetry:
    enabled: false             # Must explicitly enable
    anonymize: true            # Strip identifying info
    track_acceptance: true     # Track which suggestions users accept

analysis:
  # Toggle analysis types
  llm_analysis: true         # Primary LLM-powered analysis

  # LLM-specific toggles
  missing_docstrings: true   # Generate docstrings
  poor_comments: true        # Flag bad comments
  code_quality: true         # Best practices
  bug_detection: true        # Logic errors
  security_scan: true        # Security vulnerabilities

  # Filtering thresholds
  min_function_lines: 10     # Only analyze functions > 10 lines
  max_complexity: 10         # Flag complexity > 10
  min_docstring_score: 3.0   # Quality threshold (0-5)

  # File filters
  exclude_patterns:
    - "tests/*"              # Don't analyze test files with LLM
    - "venv/*"
    - "__pycache__/*"
    - "*.pyc"

  include_patterns:
    - "*.py"
    - "*.md"

documentation:
  enabled: true
  custom_dictionary:
    - asyncio
    - fastapi
    - gitea
    - drep
    - pytest
    - pydantic
    - sqlalchemy

database_url: sqlite:///./drep.db
```

### Config Model Updates

```python
# drep/models/config.py

from pydantic import BaseModel, Field, HttpUrl
from typing import List, Optional

class CacheConfig(BaseModel):
    """LLM response caching configuration."""
    enabled: bool = True
    directory: Path = Field(default=Path("~/.cache/drep/llm"))
    ttl_days: int = Field(default=30, ge=1)
    max_size_gb: float = Field(default=10.0, ge=0.1)
    invalidate_on_commit: bool = True

class TelemetryConfig(BaseModel):
    """Telemetry configuration (opt-in)."""
    enabled: bool = False
    anonymize: bool = True
    track_acceptance: bool = True

class LLMConfig(BaseModel):
    """LLM integration configuration."""

    enabled: bool = False
    endpoint: HttpUrl
    model: str
    api_key: Optional[str] = None

    # Model parameters
    temperature: float = Field(default=0.2, ge=0.0, le=2.0)
    max_tokens: int = Field(default=8000, ge=100, le=20000)
    timeout: int = Field(default=60, ge=10, le=300)

    # Retry configuration
    max_retries: int = Field(default=3, ge=0, le=10)
    retry_delay: int = Field(default=2, ge=1, le=60)
    exponential_backoff: bool = True

    # Concurrency & rate limiting
    max_concurrent_global: int = Field(default=5, ge=1, le=50)
    max_concurrent_per_repo: int = Field(default=3, ge=1, le=20)
    requests_per_minute: int = Field(default=60, ge=1, le=1000)
    max_tokens_per_minute: int = Field(default=100000, ge=1000)

    # Caching
    cache: CacheConfig = Field(default_factory=CacheConfig)

    # Telemetry
    telemetry: TelemetryConfig = Field(default_factory=TelemetryConfig)

class AnalysisConfig(BaseModel):
    """Analysis configuration."""

    # Toggle analysis types
    llm_analysis: bool = True

    # LLM-specific toggles
    missing_docstrings: bool = True
    poor_comments: bool = True
    code_quality: bool = True
    bug_detection: bool = True
    security_scan: bool = True

    # Filtering thresholds
    min_function_lines: int = Field(default=10, ge=1)
    max_complexity: int = Field(default=10, ge=1)
    min_docstring_score: float = Field(default=3.0, ge=0.0, le=5.0)

    # File filters
    exclude_patterns: List[str] = Field(default_factory=lambda: ["tests/*", "venv/*"])
    include_patterns: List[str] = Field(default_factory=lambda: ["*.py", "*.md"])

class Config(BaseModel):
    """Complete drep configuration."""

    gitea: GiteaConfig
    llm: Optional[LLMConfig] = None
    analysis: Optional[AnalysisConfig] = None
    documentation: DocumentationConfig
    database_url: str
```

---

## LLM Client Implementation

### Core Client

```python
# drep/llm/client.py

"""LLM client for OpenAI-compatible endpoints."""

import asyncio
import json
import logging
import re
from typing import Dict, List, Optional, Any
from datetime import datetime, timedelta

import httpx
from openai import AsyncOpenAI
from pydantic import BaseModel

logger = logging.getLogger(__name__)


class LLMResponse(BaseModel):
    """Structured LLM response."""

    content: str
    tokens_used: int
    latency_ms: float
    model: str


class RateLimitContext:
    """Context manager for rate-limited request."""

    def __init__(self, limiter: "RateLimiter", estimated_tokens: int):
        self.limiter = limiter
        self.estimated_tokens = estimated_tokens
        self.actual_tokens = 0

    async def __aenter__(self):
        # Hold semaphore for entire request duration
        await self.limiter.semaphore.acquire()

        # Wait for rate limits
        await self.limiter._check_request_rate_limit()
        await self.limiter._check_token_rate_limit(self.estimated_tokens)

        # Record estimated token usage (updated later with actual)
        self.limiter.token_times.append((datetime.now(), self.estimated_tokens))

        return self

    async def __aexit__(self, exc_type, exc_val, exc_tb):
        # Update token record with actual usage
        if self.actual_tokens > 0 and self.limiter.token_times:
            timestamp, _ = self.limiter.token_times[-1]
            self.limiter.token_times[-1] = (timestamp, self.actual_tokens)

        # Release semaphore (held for entire request)
        self.limiter.semaphore.release()
        return False

    def set_actual_tokens(self, tokens: int):
        """Update with actual token usage from response."""
        self.actual_tokens = tokens


class RateLimiter:
    """Dual-bucket rate limiter enforcing both request and token limits."""

    def __init__(
        self,
        max_concurrent: int,
        requests_per_minute: int,
        max_tokens_per_minute: int,
    ):
        """Initialize rate limiter.

        Args:
            max_concurrent: Max parallel requests (semaphore)
            requests_per_minute: Request-based rate limit
            max_tokens_per_minute: Token-based rate limit
        """
        self.semaphore = asyncio.Semaphore(max_concurrent)
        self.requests_per_minute = requests_per_minute
        self.max_tokens_per_minute = max_tokens_per_minute

        # Request tracking
        self.request_times: List[datetime] = []

        # Token tracking: (timestamp, token_count)
        self.token_times: List[tuple[datetime, int]] = []

    def request(self, estimated_tokens: int = 4000) -> RateLimitContext:
        """Create rate-limited request context.

        Args:
            estimated_tokens: Expected token usage

        Returns:
            Async context manager that enforces limits
        """
        return RateLimitContext(self, estimated_tokens)

    async def _check_request_rate_limit(self):
        """Enforce request rate limit."""
        now = datetime.now()
        cutoff = now - timedelta(minutes=1)

        # Remove old requests
        self.request_times = [t for t in self.request_times if t > cutoff]

        # Check if at limit
        if len(self.request_times) >= self.requests_per_minute:
            oldest = self.request_times[0]
            wait_time = 60 - (now - oldest).total_seconds()

            if wait_time > 0:
                logger.debug(f"Request rate limit hit, waiting {wait_time:.1f}s")
                await asyncio.sleep(wait_time)

        # Record this request
        self.request_times.append(now)

    async def _check_token_rate_limit(self, estimated_tokens: int):
        """Enforce token rate limit."""
        now = datetime.now()
        cutoff = now - timedelta(minutes=1)

        # Remove old token records
        self.token_times = [
            (t, tokens) for t, tokens in self.token_times if t > cutoff
        ]

        # Calculate current token usage
        current_tokens = sum(tokens for _, tokens in self.token_times)

        # Check if adding this request exceeds limit
        if current_tokens + estimated_tokens > self.max_tokens_per_minute:
            if self.token_times:
                oldest_time, _ = self.token_times[0]
                wait_time = 60 - (now - oldest_time).total_seconds()

                if wait_time > 0:
                    logger.info(
                        f"Token rate limit hit "
                        f"({current_tokens}/{self.max_tokens_per_minute} tokens/min), "
                        f"waiting {wait_time:.1f}s"
                    )
                    await asyncio.sleep(wait_time)


class LLMClient:
    """Client for OpenAI-compatible LLM endpoints."""

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
        max_concurrent: int = 5,
        requests_per_minute: int = 60,
        max_tokens_per_minute: int = 100000,
    ):
        """Initialize LLM client.

        Args:
            endpoint: Base URL for LLM API (e.g., https://lmstudio.localbrandonfamily.com/v1)
            model: Model identifier (e.g., qwen/qwen3-30b-a3b-2507)
            api_key: Optional API key (may not be needed for LM Studio)
            temperature: Sampling temperature (0.0-2.0)
            max_tokens: Maximum tokens per response
            timeout: Request timeout in seconds
            max_retries: Number of retries on failure
            retry_delay: Seconds between retries
            max_concurrent: Max parallel requests
            requests_per_minute: Request-based rate limit
            max_tokens_per_minute: Token-based rate limit
        """
        self.endpoint = endpoint.rstrip("/")
        self.model = model
        self.temperature = temperature
        self.max_tokens = max_tokens
        self.timeout = timeout
        self.max_retries = max_retries
        self.retry_delay = retry_delay

        # Initialize OpenAI client
        self.client = AsyncOpenAI(
            base_url=self.endpoint,
            api_key=api_key or "not-needed",  # LM Studio may not need key
            timeout=httpx.Timeout(timeout),
        )

        # Rate limiting (dual-bucket: requests + tokens)
        self.rate_limiter = RateLimiter(
            max_concurrent,
            requests_per_minute,
            max_tokens_per_minute,
        )

        # Metrics
        self.total_requests = 0
        self.total_tokens = 0
        self.failed_requests = 0

    async def analyze_code(
        self,
        system_prompt: str,
        code: str,
        temperature: Optional[float] = None,
        max_tokens: Optional[int] = None,
    ) -> LLMResponse:
        """Send code to LLM for analysis with rate limiting.

        Args:
            system_prompt: System prompt with instructions
            code: Code to analyze
            temperature: Override default temperature
            max_tokens: Override default max_tokens

        Returns:
            LLMResponse with content and metadata

        Raises:
            Exception: If all retries fail
        """
        temp = temperature if temperature is not None else self.temperature
        max_tok = max_tokens if max_tokens is not None else self.max_tokens

        # Estimate tokens (rough: 4 chars per token)
        estimated_tokens = (len(system_prompt) + len(code) + max_tok) // 4

        # Use context manager to hold semaphore for entire request
        async with self.rate_limiter.request(estimated_tokens) as ctx:
            start_time = datetime.now()

            for attempt in range(self.max_retries):
                try:
                    response = await self.client.chat.completions.create(
                        model=self.model,
                        messages=[
                            {"role": "system", "content": system_prompt},
                            {"role": "user", "content": code},
                        ],
                        temperature=temp,
                        max_tokens=max_tok,
                    )

                    end_time = datetime.now()
                    latency_ms = (end_time - start_time).total_seconds() * 1000

                    content = response.choices[0].message.content
                    tokens_used = response.usage.total_tokens

                    # Update context with actual token usage
                    ctx.set_actual_tokens(tokens_used)

                    # Update metrics
                    self.total_requests += 1
                    self.total_tokens += tokens_used

                    logger.debug(
                        f"LLM request successful: {tokens_used} tokens, "
                        f"{latency_ms:.0f}ms latency"
                    )

                    return LLMResponse(
                        content=content,
                        tokens_used=tokens_used,
                        latency_ms=latency_ms,
                        model=self.model,
                    )

                except Exception as e:
                    self.failed_requests += 1

                    if attempt < self.max_retries - 1:
                        logger.warning(
                            f"LLM request failed (attempt {attempt + 1}/{self.max_retries}): {e}"
                        )
                        await asyncio.sleep(self.retry_delay * (attempt + 1))
                    else:
                        logger.error(f"LLM request failed after {self.max_retries} attempts: {e}")
                        raise

    async def analyze_code_json(
        self,
        system_prompt: str,
        code: str,
        expected_schema: Optional[type[BaseModel]] = None,
    ) -> Dict[str, Any]:
        """Send code to LLM and parse JSON response with robust error recovery.

        Handles:
        - Markdown code fences
        - Truncated responses (missing closing brackets)
        - Malformed JSON (trailing commas, single quotes)
        - Fuzzy inference when JSON is unrecoverable

        Args:
            system_prompt: System prompt (should request JSON output)
            code: Code to analyze
            expected_schema: Optional Pydantic model to validate response

        Returns:
            Parsed JSON response as dictionary

        Raises:
            ValueError: If all parse strategies fail after retries
        """
        last_error = None

        for attempt in range(3):  # Try up to 3 times
            response = await self.analyze_code(system_prompt, code)
            content = response.content.strip()

            # Strategy 1: Extract from markdown fences
            if "```json" in content or "```" in content:
                match = re.search(r'```json\s*(.*?)\s*```', content, re.DOTALL)
                if not match:
                    match = re.search(r'```\s*(.*?)\s*```', content, re.DOTALL)
                if match:
                    content = match.group(1).strip()

            # Strategy 2: Try direct parse
            try:
                data = json.loads(content)
                if expected_schema:
                    expected_schema(**data)  # Validate
                return data
            except json.JSONDecodeError:
                pass

            # Strategy 3: Fix common errors
            cleaned = content
            cleaned = re.sub(r',(\s*[}\]])', r'\1', cleaned)  # Trailing commas
            cleaned = cleaned.replace("'", '"')  # Single quotes
            try:
                data = json.loads(cleaned)
                if expected_schema:
                    expected_schema(**data)
                logger.warning("Recovered malformed JSON with cleanup")
                return data
            except json.JSONDecodeError:
                pass

            # Strategy 4: Recover truncated JSON
            open_braces = content.count('{')
            close_braces = content.count('}')
            open_brackets = content.count('[')
            close_brackets = content.count(']')

            if open_braces > close_braces or open_brackets > close_brackets:
                recovered = content
                recovered += ']' * (open_brackets - close_brackets)
                recovered += '}' * (open_braces - close_braces)
                try:
                    data = json.loads(recovered)
                    if expected_schema:
                        expected_schema(**data)
                    logger.warning(f"Recovered truncated JSON (added {open_braces - close_braces} braces)")
                    return data
                except json.JSONDecodeError as e:
                    last_error = e

            # Strategy 5: Fuzzy inference (if schema provided)
            if expected_schema and attempt == 2:  # Last attempt
                logger.warning("Attempting fuzzy inference from malformed response")
                inferred = self._fuzzy_inference(content, expected_schema)
                if inferred:
                    logger.warning(f"Using fuzzy inference (low confidence)")
                    return inferred

            # Retry with adjusted prompt
            if attempt < 2:
                logger.warning(f"Parse attempt {attempt + 1} failed, retrying...")
                system_prompt += "\n\nIMPORTANT: Return ONLY valid JSON, no explanation."
                await asyncio.sleep(1)

        # All strategies failed
        raise ValueError(
            f"Failed to parse LLM response after 3 attempts. Last error: {last_error}"
        )

    def _fuzzy_inference(self, content: str, schema: type[BaseModel]) -> Optional[Dict[str, Any]]:
        """Attempt to infer structure from malformed response using regex patterns.

        This is a last-resort fallback with low confidence.
        """
        field_names = schema.__fields__.keys()
        extracted = {}

        for field_name in field_names:
            # Try multiple patterns to extract field values
            patterns = [
                rf'"{field_name}":\s*"([^"]*)"',  # JSON string
                rf'"{field_name}":\s*(\d+\.?\d*)',  # JSON number
                rf'{field_name}:\s*([^\n,}}]+)',  # Unquoted value
            ]

            for pattern in patterns:
                match = re.search(pattern, content, re.IGNORECASE)
                if match:
                    extracted[field_name] = match.group(1).strip()
                    break

        if extracted and len(extracted) > len(field_names) / 2:  # Got >50% of fields
            logger.info(f"Fuzzy inference extracted: {list(extracted.keys())}")
            return extracted

        return None

    async def batch_analyze(
        self,
        tasks: List[tuple[str, str]],
    ) -> List[LLMResponse]:
        """Analyze multiple code samples in parallel.

        Args:
            tasks: List of (system_prompt, code) tuples

        Returns:
            List of LLMResponse objects
        """
        results = await asyncio.gather(
            *[self.analyze_code(prompt, code) for prompt, code in tasks],
            return_exceptions=True,
        )

        # Filter out exceptions and log them
        successful = []
        for i, result in enumerate(results):
            if isinstance(result, Exception):
                logger.error(f"Batch task {i} failed: {result}")
            else:
                successful.append(result)

        return successful

    def get_metrics(self) -> Dict[str, Any]:
        """Get client metrics.

        Returns:
            Dictionary with usage statistics
        """
        return {
            "total_requests": self.total_requests,
            "total_tokens": self.total_tokens,
            "failed_requests": self.failed_requests,
            "success_rate": (
                (self.total_requests - self.failed_requests) / self.total_requests
                if self.total_requests > 0
                else 0.0
            ),
            "avg_tokens_per_request": (
                self.total_tokens / self.total_requests
                if self.total_requests > 0
                else 0
            ),
        }

    async def close(self):
        """Close HTTP client."""
        await self.client.close()
```

### Usage Example

```python
# Example usage of LLM client

from drep.llm.client import LLMClient

# Initialize client
llm = LLMClient(
    endpoint="https://lmstudio.localbrandonfamily.com/v1",
    model="qwen/qwen3-30b-a3b-2507",
    max_tokens=8000,
)

# Analyze code
code = '''
def process_data(data):
    result = []
    for item in data:
        if item > 0:
            result.append(item * 2)
    return result
'''

prompt = """Analyze this Python function for:
1. Missing docstring
2. Type hints
3. Edge cases

Return JSON: {"issues": [...], "suggestions": [...]}
"""

response = await llm.analyze_code_json(prompt, code)
print(response)

# Get metrics
metrics = llm.get_metrics()
print(f"Total requests: {metrics['total_requests']}")
print(f"Total tokens: {metrics['total_tokens']}")

# Clean up
await llm.close()
```

---

## Code Analysis Pipeline

### Analysis Flow

```
┌──────────────────────────────────────────────────────────┐
│ 1. File Discovery                                        │
│    RepositoryScanner finds Python files                  │
└────────────┬─────────────────────────────────────────────┘
             │
┌────────────▼─────────────────────────────────────────────┐
│ 2. AST Parsing                                           │
│    Extract functions, classes, imports                   │
└────────────┬─────────────────────────────────────────────┘
             │
┌────────────▼─────────────────────────────────────────────┐
│ 3. Function Filtering                                    │
│    • Min 10 lines                                        │
│    • Complexity > threshold                              │
│    • Public functions only                               │
└────────────┬─────────────────────────────────────────────┘
             │
┌────────────▼─────────────────────────────────────────────┐
│ 4. LLM Analysis (Intelligent)                           │
│    • Missing docstrings → Generate                       │
│    • Poor comments → Suggest improvements                │
│    • Code quality → Find bugs                            │
│    • Security → Detect vulnerabilities                   │
└────────────┬─────────────────────────────────────────────┘
             │
┌────────────▼─────────────────────────────────────────────┐
│ 5. Finding Aggregation                                   │
│    • Deduplicate                                         │
│    • Prioritize by severity                              │
│    • Group related issues                                │
└────────────┬─────────────────────────────────────────────┘
             │
┌────────────▼─────────────────────────────────────────────┐
│ 6. Issue Creation                                        │
│    • Create Gitea issues                                 │
│    • Include LLM suggestions                             │
│    • Link to specific lines                              │
└──────────────────────────────────────────────────────────┘
```

### AST Extraction

```python
# drep/analyzers/ast_utils.py

"""AST utilities for Python code analysis."""

import ast
from dataclasses import dataclass
from typing import List, Optional


@dataclass
class FunctionInfo:
    """Information about a Python function."""

    name: str
    line_start: int
    line_end: int
    docstring: Optional[str]
    signature: str
    complexity: int
    is_public: bool
    has_type_hints: bool
    body: str


def extract_functions(file_path: str, source_code: str) -> List[FunctionInfo]:
    """Extract function information from Python source.

    Args:
        file_path: Path to file (for error messages)
        source_code: Python source code

    Returns:
        List of FunctionInfo objects
    """
    try:
        tree = ast.parse(source_code)
    except SyntaxError as e:
        logger.error(f"Syntax error in {file_path}: {e}")
        return []

    functions = []

    for node in ast.walk(tree):
        if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
            # Extract function info
            func_info = FunctionInfo(
                name=node.name,
                line_start=node.lineno,
                line_end=node.end_lineno,
                docstring=ast.get_docstring(node),
                signature=_build_signature(node),
                complexity=_calculate_complexity(node),
                is_public=not node.name.startswith("_"),
                has_type_hints=_has_type_hints(node),
                body=_extract_body(source_code, node.lineno, node.end_lineno),
            )
            functions.append(func_info)

    return functions


def _build_signature(node: ast.FunctionDef) -> str:
    """Build function signature string."""
    args = []

    for arg in node.args.args:
        annotation = ast.unparse(arg.annotation) if arg.annotation else ""
        arg_str = f"{arg.arg}: {annotation}" if annotation else arg.arg
        args.append(arg_str)

    returns = ast.unparse(node.returns) if node.returns else ""
    return_str = f" -> {returns}" if returns else ""

    return f"def {node.name}({', '.join(args)}){return_str}:"


def _calculate_complexity(node: ast.FunctionDef) -> int:
    """Calculate cyclomatic complexity."""
    complexity = 1  # Base complexity

    for child in ast.walk(node):
        if isinstance(child, (ast.If, ast.While, ast.For, ast.AsyncFor)):
            complexity += 1
        elif isinstance(child, ast.BoolOp):
            complexity += len(child.values) - 1
        elif isinstance(child, (ast.ExceptHandler,)):
            complexity += 1

    return complexity


def _has_type_hints(node: ast.FunctionDef) -> bool:
    """Check if function has type hints."""
    # Check return type
    if node.returns:
        return True

    # Check argument types
    for arg in node.args.args:
        if arg.annotation:
            return True

    return False


def _extract_body(source_code: str, start_line: int, end_line: int) -> str:
    """Extract function body from source."""
    lines = source_code.splitlines()
    body_lines = lines[start_line - 1:end_line]
    return "\n".join(body_lines)
```

---

## Analyzer Components

### 1. Code Quality Analyzer

```python
# drep/analyzers/code_quality.py

"""LLM-powered code quality analyzer."""

import logging
from typing import List
from pydantic import BaseModel

from drep.llm.client import LLMClient
from drep.analyzers.ast_utils import FunctionInfo
from drep.models.finding import Finding, FindingSeverity

logger = logging.getLogger(__name__)


class CodeIssue(BaseModel):
    """Single code quality issue."""

    type: str  # bug, security, style, performance
    severity: str  # critical, high, medium, low
    line: int
    description: str
    suggestion: str


class CodeQualityResult(BaseModel):
    """Result from code quality analysis."""

    issues: List[CodeIssue]
    overall_score: float  # 0-10


class CodeQualityAnalyzer:
    """Analyzes Python code for quality issues using LLM."""

    ANALYSIS_PROMPT = """You are a senior Python engineer conducting a code review. Analyze this function for:

**1. Logic Errors & Bugs**
- Off-by-one errors
- Null/None handling
- Edge cases not handled
- Infinite loops
- Resource leaks

**2. Security Vulnerabilities**
- SQL injection
- Command injection
- Path traversal
- Unsafe deserialization
- Hardcoded secrets

**3. Code Quality**
- Missing error handling
- Overly complex logic
- Dead code
- Code duplication
- Poor variable names

**4. Python Best Practices**
- PEP 8 violations
- Missing type hints
- Mutable default arguments
- Bare except clauses
- Using deprecated APIs

**Instructions:**
- Focus on real issues, not style preferences
- Provide specific line numbers
- Suggest concrete fixes
- Use severity: critical, high, medium, low

**Output Format:**
Return JSON only (no markdown, no explanation):
```json
{
  "issues": [
    {
      "type": "bug",
      "severity": "high",
      "line": 5,
      "description": "Potential IndexError when list is empty",
      "suggestion": "Add check: if not data: return []"
    }
  ],
  "overall_score": 7.5
}
```

**Function to Analyze:**
```python
{code}
```"""

    def __init__(self, llm_client: LLMClient, min_complexity: int = 5):
        """Initialize code quality analyzer.

        Args:
            llm_client: LLM client instance
            min_complexity: Minimum complexity to analyze
        """
        self.llm = llm_client
        self.min_complexity = min_complexity

    async def analyze_function(
        self,
        func_info: FunctionInfo,
        file_path: str,
    ) -> List[Finding]:
        """Analyze a single function for quality issues.

        Args:
            func_info: Function information from AST
            file_path: Path to source file

        Returns:
            List of Finding objects
        """
        # Skip simple functions
        if func_info.complexity < self.min_complexity:
            logger.debug(f"Skipping {func_info.name} (complexity {func_info.complexity})")
            return []

        # Prepare prompt
        prompt = self.ANALYSIS_PROMPT.format(code=func_info.body)

        try:
            # Call LLM
            result_json = await self.llm.analyze_code_json(
                system_prompt=prompt,
                code=func_info.body,
                schema=CodeQualityResult,
            )

            result = CodeQualityResult(**result_json)

            # Convert to Finding objects
            findings = []
            for issue in result.issues:
                finding = Finding(
                    type=f"code_quality_{issue.type}",
                    severity=self._map_severity(issue.severity),
                    file_path=file_path,
                    line=issue.line,
                    column=0,
                    message=issue.description,
                    suggestion=issue.suggestion,
                    context=f"Function: {func_info.name}",
                )
                findings.append(finding)

            logger.info(
                f"Analyzed {func_info.name}: {len(findings)} issues, "
                f"score {result.overall_score}/10"
            )

            return findings

        except Exception as e:
            logger.error(f"Failed to analyze {func_info.name}: {e}")
            return []

    async def analyze_file(
        self,
        file_path: str,
        functions: List[FunctionInfo],
    ) -> List[Finding]:
        """Analyze all functions in a file.

        Args:
            file_path: Path to source file
            functions: List of functions from AST

        Returns:
            Aggregated list of findings
        """
        # Filter functions to analyze
        to_analyze = [
            f for f in functions
            if f.complexity >= self.min_complexity and f.is_public
        ]

        if not to_analyze:
            logger.debug(f"No functions to analyze in {file_path}")
            return []

        logger.info(f"Analyzing {len(to_analyze)} functions in {file_path}")

        # Analyze in parallel
        all_findings = []
        for func_info in to_analyze:
            findings = await self.analyze_function(func_info, file_path)
            all_findings.extend(findings)

        return all_findings

    def _map_severity(self, llm_severity: str) -> FindingSeverity:
        """Map LLM severity to Finding severity."""
        mapping = {
            "critical": FindingSeverity.ERROR,
            "high": FindingSeverity.ERROR,
            "medium": FindingSeverity.WARNING,
            "low": FindingSeverity.INFO,
        }
        return mapping.get(llm_severity.lower(), FindingSeverity.INFO)
```

### 2. Docstring Generator

```python
# drep/analyzers/docstring_generator.py

"""LLM-powered docstring generator."""

import logging
from typing import Optional
from pydantic import BaseModel

from drep.llm.client import LLMClient
from drep.analyzers.ast_utils import FunctionInfo
from drep.models.finding import Finding, FindingSeverity

logger = logging.getLogger(__name__)


class DocstringResult(BaseModel):
    """Result from docstring generation."""

    docstring: str
    confidence: float  # 0.0-1.0


class DocstringGenerator:
    """Generates missing docstrings using LLM."""

    DOCSTRING_PROMPT = """You are a Python documentation expert. Generate a Google-style docstring for this function.

**Requirements:**
1. Brief one-line summary
2. Detailed description (if function is complex)
3. Args: List each parameter with type and description
4. Returns: Describe return value with type
5. Raises: List exceptions that may be raised

**Style:**
- Use Google docstring format
- Be concise but complete
- Infer types from code if not annotated
- Describe what the function DOES, not how

**Output Format:**
Return JSON only:
```json
{
  "docstring": "Brief summary.\\n\\nDetailed description.\\n\\nArgs:\\n    param1: Description.\\n    param2: Description.\\n\\nReturns:\\n    Description of return value.\\n\\nRaises:\\n    ValueError: When...",
  "confidence": 0.95
}
```

**Function:**
```python
{code}
```"""

    def __init__(self, llm_client: LLMClient, min_lines: int = 10):
        """Initialize docstring generator.

        Args:
            llm_client: LLM client instance
            min_lines: Minimum function lines to generate docstring
        """
        self.llm = llm_client
        self.min_lines = min_lines

    async def generate_docstring(
        self,
        func_info: FunctionInfo,
        file_path: str,
    ) -> Optional[Finding]:
        """Generate docstring for a function missing one.

        Args:
            func_info: Function information from AST
            file_path: Path to source file

        Returns:
            Finding with generated docstring, or None if skipped
        """
        # Skip if already has docstring
        if func_info.docstring:
            return None

        # Skip if function is too simple
        line_count = func_info.line_end - func_info.line_start
        if line_count < self.min_lines:
            return None

        # Skip private functions
        if not func_info.is_public:
            return None

        # Prepare prompt
        prompt = self.DOCSTRING_PROMPT.format(code=func_info.body)

        try:
            # Call LLM
            result_json = await self.llm.analyze_code_json(
                system_prompt=prompt,
                code=func_info.body,
                schema=DocstringResult,
            )

            result = DocstringResult(**result_json)

            # Create finding with suggestion
            finding = Finding(
                type="missing_docstring",
                severity=FindingSeverity.WARNING,
                file_path=file_path,
                line=func_info.line_start,
                column=0,
                message=f"Function '{func_info.name}' is missing a docstring",
                suggestion=self._format_docstring_suggestion(
                    func_info.name,
                    result.docstring
                ),
                context=f"Generated with {result.confidence:.0%} confidence",
            )

            logger.info(
                f"Generated docstring for {func_info.name} "
                f"(confidence: {result.confidence:.0%})"
            )

            return finding

        except Exception as e:
            logger.error(f"Failed to generate docstring for {func_info.name}: {e}")
            return None

    async def analyze_file(
        self,
        file_path: str,
        functions: List[FunctionInfo],
    ) -> List[Finding]:
        """Generate docstrings for all functions in a file.

        Args:
            file_path: Path to source file
            functions: List of functions from AST

        Returns:
            List of findings with generated docstrings
        """
        findings = []

        for func_info in functions:
            finding = await self.generate_docstring(func_info, file_path)
            if finding:
                findings.append(finding)

        return findings

    def _format_docstring_suggestion(self, func_name: str, docstring: str) -> str:
        """Format docstring suggestion for issue."""
        return f"""Add this docstring to `{func_name}()`:

```python
def {func_name}(...):
    \"\"\"
    {docstring}
    \"\"\"
```"""
```

### 3. PR Review Analyzer

```python
# drep/analyzers/pr_review.py

"""LLM-powered PR review analyzer."""

import logging
from typing import List, Dict, Any
from pydantic import BaseModel

from drep.llm.client import LLMClient
from drep.adapters.gitea import GiteaAdapter

logger = logging.getLogger(__name__)


class ReviewComment(BaseModel):
    """Single review comment."""

    file_path: str
    line: int
    comment: str
    severity: str  # info, suggestion, warning, critical


class PRReviewResult(BaseModel):
    """Result from PR review."""

    comments: List[ReviewComment]
    summary: str
    approve: bool


class PRReviewAnalyzer:
    """Analyzes PR diffs using LLM."""

    REVIEW_PROMPT = """You are a senior Python engineer reviewing a pull request. Analyze this code change for:

**1. Correctness**
- Does the code do what it's supposed to?
- Are there logic errors or bugs?
- Edge cases handled?

**2. Best Practices**
- Follows Python conventions (PEP 8)?
- Proper error handling?
- Good variable names?
- Type hints present?

**3. Testing**
- Are tests included?
- Are tests comprehensive?
- Edge cases tested?

**4. Documentation**
- Docstrings added/updated?
- Comments explain why, not what?
- README updated if needed?

**5. Security & Performance**
- Any security vulnerabilities?
- Performance concerns?
- Resource leaks?

**Context:**
PR #{pr_number}: {pr_title}
Description: {pr_description}

**Changed Files:**
{diff}

**Instructions:**
- Be constructive, not just critical
- Suggest specific improvements
- Highlight good changes too
- Consider the PR's stated goal

**Output Format:**
Return JSON only:
```json
{
  "comments": [
    {
      "file_path": "src/module.py",
      "line": 42,
      "comment": "Consider adding error handling here...",
      "severity": "suggestion"
    }
  ],
  "summary": "Overall assessment of PR...",
  "approve": true
}
```"""

    def __init__(
        self,
        llm_client: LLMClient,
        gitea_adapter: GiteaAdapter,
    ):
        """Initialize PR review analyzer.

        Args:
            llm_client: LLM client instance
            gitea_adapter: Gitea adapter for API calls
        """
        self.llm = llm_client
        self.gitea = gitea_adapter

    async def review_pr(
        self,
        owner: str,
        repo: str,
        pr_number: int,
    ) -> PRReviewResult:
        """Review a pull request.

        Args:
            owner: Repository owner
            repo: Repository name
            pr_number: PR number

        Returns:
            PRReviewResult with comments and summary
        """
        # Fetch PR details
        pr_data = await self.gitea.get_pr(owner, repo, pr_number)

        # Get PR diff
        diff = await self.gitea.get_pr_diff(owner, repo, pr_number)

        # Truncate diff if too large (max 8k tokens ≈ 32k chars)
        if len(diff) > 30000:
            logger.warning(f"PR diff too large ({len(diff)} chars), truncating")
            diff = diff[:30000] + "\n\n... [truncated] ..."

        # Prepare prompt
        prompt = self.REVIEW_PROMPT.format(
            pr_number=pr_number,
            pr_title=pr_data["title"],
            pr_description=pr_data["body"] or "(no description)",
            diff=diff,
        )

        # Call LLM (use max tokens since PR review needs detail)
        result_json = await self.llm.analyze_code_json(
            system_prompt=prompt,
            code="",  # Diff is in prompt
            schema=PRReviewResult,
        )

        result = PRReviewResult(**result_json)

        logger.info(
            f"Reviewed PR #{pr_number}: {len(result.comments)} comments, "
            f"approve={result.approve}"
        )

        return result

    async def post_review(
        self,
        owner: str,
        repo: str,
        pr_number: int,
        result: PRReviewResult,
    ):
        """Post review comments to PR.

        Args:
            owner: Repository owner
            repo: Repository name
            pr_number: PR number
            result: Review result to post
        """
        # Post summary as PR comment
        summary_body = f"""## 🤖 drep AI Code Review

{result.summary}

**Recommendation:** {"✅ Approve" if result.approve else "🔍 Needs Changes"}

---
*Generated by drep using {self.llm.model}*
"""

        await self.gitea.create_pr_comment(
            owner=owner,
            repo=repo,
            pr_number=pr_number,
            body=summary_body,
        )

        # Post inline comments
        for comment in result.comments:
            await self.gitea.create_pr_review_comment(
                owner=owner,
                repo=repo,
                pr_number=pr_number,
                file_path=comment.file_path,
                line=comment.line,
                body=f"**{comment.severity.upper()}**: {comment.comment}",
            )

        logger.info(f"Posted {len(result.comments)} review comments to PR #{pr_number}")
```

---

## Prompt Engineering

### Best Practices

1. **Be Specific**: Tell the LLM exactly what to look for
2. **Use Examples**: Show desired output format
3. **Set Context**: Explain the LLM's role (senior engineer, documentation expert)
4. **Request Structured Output**: Always ask for JSON
5. **Limit Scope**: Focus on one task per prompt
6. **Provide Schema**: Include expected JSON structure
7. **Handle Errors**: Plan for invalid responses

### Prompt Templates

#### 1. Docstring Generation

```python
DOCSTRING_PROMPT = """You are a Python documentation expert. Generate a Google-style docstring for this function.

**Requirements:**
- Brief one-line summary
- Args with types and descriptions
- Returns with type and description
- Raises for exceptions

**Example Output:**
```json
{
  "docstring": "Calculate the sum of two numbers.\\n\\nArgs:\\n    a (int): First number\\n    b (int): Second number\\n\\nReturns:\\n    int: Sum of a and b",
  "confidence": 0.95
}
```

**Function to Document:**
```python
{code}
```

Return JSON only (no explanation, no markdown fences)."""
```

#### 2. Bug Detection

```python
BUG_DETECTION_PROMPT = """You are a senior Python engineer specializing in bug detection. Analyze this code for potential bugs.

**Look For:**
- Off-by-one errors
- Null/None handling issues
- Type mismatches
- Edge cases not handled
- Resource leaks
- Race conditions

**Do Not Report:**
- Style issues (handled by linters)
- Minor optimizations
- Subjective preferences

**Example Output:**
```json
{
  "bugs": [
    {
      "line": 15,
      "type": "index_error",
      "description": "Accessing list without checking length",
      "fix": "Add check: if index < len(items):",
      "confidence": 0.9
    }
  ]
}
```

**Code to Analyze:**
```python
{code}
```

Return JSON only."""
```

#### 3. Security Scan

```python
SECURITY_SCAN_PROMPT = """You are a security researcher analyzing Python code for vulnerabilities.

**Vulnerabilities to Check:**
1. SQL Injection (unsafe string formatting in queries)
2. Command Injection (unsafe shell command construction)
3. Path Traversal (unchecked file paths)
4. Unsafe Deserialization (pickle, yaml.load)
5. Hardcoded Secrets (passwords, API keys, tokens)
6. Weak Crypto (MD5, SHA1 for passwords)

**Severity Levels:**
- critical: Exploitable vulnerability
- high: Likely exploitable with effort
- medium: Requires specific conditions
- low: Theoretical or edge case

**Example Output:**
```json
{
  "vulnerabilities": [
    {
      "type": "sql_injection",
      "severity": "critical",
      "line": 42,
      "description": "User input directly in SQL query",
      "exploit_scenario": "Attacker can inject: ' OR '1'='1",
      "fix": "Use parameterized queries: cursor.execute('SELECT * FROM users WHERE id=?', (user_id,))"
    }
  ]
}
```

**Code to Scan:**
```python
{code}
```

Return JSON only."""
```

#### 4. PR Summary

```python
PR_SUMMARY_PROMPT = """You are a technical writer summarizing a pull request for team review.

**Create a Summary With:**
1. What Changed: High-level overview
2. Why: Purpose and motivation
3. Key Changes: List of important modifications
4. Risks: Potential breaking changes or concerns
5. Testing: What tests were added/updated

**Tone:**
- Professional but friendly
- Concise (3-5 sentences for "What Changed")
- Highlight important details

**Example Output:**
```json
{
  "summary": "## What Changed\\n\\nRefactored authentication system to use JWT tokens instead of session cookies...\\n\\n## Why\\n\\nSession cookies don't work well with our microservices architecture...\\n\\n## Key Changes\\n- Added JWT generation in auth.py\\n- Updated middleware...\\n\\n## Risks\\n- Breaking change for existing clients\\n- Requires database migration\\n\\n## Testing\\n- Added 15 tests for token validation\\n- Manual testing with Postman"
}
```

**PR Details:**
Title: {pr_title}
Description: {pr_description}
Files Changed: {file_count}

**Diff:**
{diff}

Return JSON only."""
```

### Response Parsing

```python
# drep/llm/response_parser.py

"""Utilities for parsing LLM responses."""

import json
import logging
import re
from typing import Dict, Any, Optional

logger = logging.getLogger(__name__)


def extract_json_from_response(response: str) -> Optional[Dict[str, Any]]:
    """Extract JSON from LLM response.

    Handles:
    - Plain JSON
    - JSON in markdown code fences
    - JSON with surrounding text

    Args:
        response: Raw LLM response

    Returns:
        Parsed JSON dictionary, or None if invalid
    """
    # Remove markdown code fences
    cleaned = response.strip()

    # Try removing ```json ... ```
    if "```json" in cleaned:
        match = re.search(r'```json\s*(.*?)\s*```', cleaned, re.DOTALL)
        if match:
            cleaned = match.group(1)

    # Try removing ``` ... ```
    elif cleaned.startswith("```"):
        cleaned = re.sub(r'^```\w*\s*', '', cleaned)
        cleaned = re.sub(r'\s*```$', '', cleaned)

    # Try to find JSON object anywhere in response
    json_match = re.search(r'(\{.*\})', cleaned, re.DOTALL)
    if json_match:
        cleaned = json_match.group(1)

    # Parse JSON
    try:
        return json.loads(cleaned)
    except json.JSONDecodeError as e:
        logger.error(f"Failed to parse JSON: {e}")
        logger.debug(f"Raw response: {response}")
        return None


def validate_json_schema(
    data: Dict[str, Any],
    required_fields: list[str],
) -> bool:
    """Validate JSON has required fields.

    Args:
        data: Parsed JSON data
        required_fields: List of required field names

    Returns:
        True if all fields present, False otherwise
    """
    for field in required_fields:
        if field not in data:
            logger.warning(f"Missing required field: {field}")
            return False

    return True
```

---

## Issue Generation

### Enhanced Issue Format

Instead of simple typo issues, create rich, actionable issues:

```markdown
[drep] Missing Docstring: process_results()

**Type:** Documentation
**Severity:** Medium
**File:** `src/analyzer.py:145-203`

---

## Issue

Function `process_results()` lacks documentation, making it difficult for other developers to understand its purpose, parameters, and return value.

## Context

```python
def process_results(self, data: List[Dict]) -> AnalysisResult:
    # 58 lines of code without docstring
    ...
```

## Suggested Fix

Add a comprehensive docstring:

```python
def process_results(self, data: List[Dict]) -> AnalysisResult:
    """Process analysis results and generate report.

    Takes raw analysis data from the scanner and transforms it into
    a structured AnalysisResult object with findings, statistics, and
    recommendations.

    Args:
        data: List of analysis results from scanner. Each dict should contain:
            - file_path (str): Path to analyzed file
            - findings (List[Dict]): Found issues
            - metadata (Dict): Scan metadata

    Returns:
        AnalysisResult containing:
            - findings: Processed Finding objects
            - stats: Aggregated statistics
            - recommendations: Suggested actions

    Raises:
        ValueError: If data is empty or has invalid format
        KeyError: If required fields missing from data dicts

    Example:
        >>> analyzer = Analyzer()
        >>> data = [{"file_path": "test.py", "findings": [...]}]
        >>> result = analyzer.process_results(data)
        >>> print(result.stats.total_findings)
        42
    """
```

## Why This Matters

- **Maintainability**: New team members can understand the function faster
- **Type Safety**: Clarifies expected data structure
- **Error Handling**: Documents exception conditions
- **Examples**: Shows how to use the function

---

*Analyzed by drep using Qwen3-30B (confidence: 94%)*
```

### Issue Templates

```python
# drep/core/issue_templates.py

"""Templates for generating rich Gitea issues."""

from typing import Dict, Any


DOCSTRING_ISSUE_TEMPLATE = """[drep] Missing Docstring: {function_name}

**Type:** Documentation
**Severity:** {severity}
**File:** `{file_path}:{line_start}-{line_end}`

---

## Issue

Function `{function_name}()` lacks documentation, making it difficult for other developers to understand its purpose, parameters, and return value.

## Context

```python
{function_signature}
    # {line_count} lines of code without docstring
    ...
```

## Suggested Fix

{suggested_docstring}

## Why This Matters

- **Maintainability**: New team members can understand the function faster
- **Type Safety**: Clarifies expected data structure
- **Error Handling**: Documents exception conditions
- **Examples**: Shows how to use the function

---

*Analyzed by drep using {model_name} (confidence: {confidence}%)*
"""


BUG_ISSUE_TEMPLATE = """[drep] Potential Bug: {bug_type}

**Type:** Code Quality
**Severity:** {severity}
**File:** `{file_path}:{line}`

---

## Issue

{description}

## Vulnerable Code

```python
{code_snippet}
```

## Problem

{detailed_explanation}

## Suggested Fix

```python
{suggested_fix}
```

## Testing Suggestion

```python
# Test case to verify the fix
{test_suggestion}
```

---

*Detected by drep using {model_name}*
"""


SECURITY_ISSUE_TEMPLATE = """[drep] 🔒 Security: {vulnerability_type}

**Type:** Security Vulnerability
**Severity:** {severity} ⚠️
**File:** `{file_path}:{line}`

---

## Vulnerability

{description}

## Vulnerable Code

```python
{code_snippet}
```

## Exploit Scenario

{exploit_scenario}

## Fix

```python
{suggested_fix}
```

## References

{cve_or_references}

---

*Detected by drep security scanner using {model_name}*
"""


def generate_issue_body(
    template: str,
    **kwargs: Dict[str, Any],
) -> str:
    """Generate issue body from template.

    Args:
        template: Issue template string
        **kwargs: Template variables

    Returns:
        Formatted issue body
    """
    return template.format(**kwargs)
```

---

## Testing Strategy

### Unit Tests

```python
# tests/unit/test_llm_client.py

"""Tests for LLM client."""

import pytest
from unittest.mock import AsyncMock, MagicMock
from drep.llm.client import LLMClient, LLMResponse


@pytest.mark.asyncio
async def test_llm_client_analyze_code():
    """Test basic code analysis."""
    client = LLMClient(
        endpoint="https://test.local/v1",
        model="test-model",
    )

    # Mock OpenAI client
    mock_response = MagicMock()
    mock_response.choices = [MagicMock(message=MagicMock(content="Test response"))]
    mock_response.usage = MagicMock(total_tokens=100)

    client.client.chat.completions.create = AsyncMock(return_value=mock_response)

    result = await client.analyze_code("Test prompt", "def foo(): pass")

    assert isinstance(result, LLMResponse)
    assert result.content == "Test response"
    assert result.tokens_used == 100

    await client.close()


@pytest.mark.asyncio
async def test_llm_client_retry_on_failure():
    """Test retry logic on failure."""
    client = LLMClient(
        endpoint="https://test.local/v1",
        model="test-model",
        max_retries=3,
    )

    # Mock failures then success
    mock_create = AsyncMock(side_effect=[
        Exception("Connection error"),
        Exception("Timeout"),
        MagicMock(
            choices=[MagicMock(message=MagicMock(content="Success"))],
            usage=MagicMock(total_tokens=50),
        ),
    ])

    client.client.chat.completions.create = mock_create

    result = await client.analyze_code("Test", "code")

    assert result.content == "Success"
    assert mock_create.call_count == 3

    await client.close()
```

### Integration Tests

```python
# tests/integration/test_llm_integration.py

"""Integration tests for LLM analysis."""

import pytest
from drep.llm.client import LLMClient
from drep.analyzers.code_quality import CodeQualityAnalyzer
from drep.analyzers.ast_utils import extract_functions


@pytest.mark.integration
@pytest.mark.asyncio
async def test_analyze_real_function():
    """Test analyzing a real Python function."""
    # Requires LM Studio running
    client = LLMClient(
        endpoint="https://lmstudio.localbrandonfamily.com/v1",
        model="qwen/qwen3-30b-a3b-2507",
    )

    analyzer = CodeQualityAnalyzer(client, min_complexity=2)

    # Test code with intentional issues
    test_code = '''
def process_items(items):
    result = []
    for i in range(len(items) + 1):  # Bug: off-by-one
        result.append(items[i])
    return result
'''

    functions = extract_functions("test.py", test_code)
    findings = await analyzer.analyze_file("test.py", functions)

    # Should detect the off-by-one error
    assert len(findings) > 0
    assert any("index" in f.message.lower() for f in findings)

    await client.close()
```

### Manual Testing Script

```python
# scripts/test_llm.py

"""Manual testing script for LLM integration."""

import asyncio
import sys
from pathlib import Path

# Add parent directory to path
sys.path.insert(0, str(Path(__file__).parent.parent))

from drep.llm.client import LLMClient
from drep.analyzers.code_quality import CodeQualityAnalyzer
from drep.analyzers.ast_utils import extract_functions


async def main():
    """Test LLM integration."""
    # Initialize client
    print("Connecting to LM Studio...")
    client = LLMClient(
        endpoint="https://lmstudio.localbrandonfamily.com/v1",
        model="qwen/qwen3-30b-a3b-2507",
        max_tokens=8000,
    )

    # Test connection
    print("Testing connection...")
    try:
        response = await client.analyze_code(
            "Say 'hello' in JSON format: {\"message\": \"hello\"}",
            "",
        )
        print(f"✓ Connection successful: {response.content}")
    except Exception as e:
        print(f"✗ Connection failed: {e}")
        return

    # Test code quality analyzer
    print("\nTesting code quality analyzer...")
    analyzer = CodeQualityAnalyzer(client, min_complexity=2)

    test_code = '''
def calculate_total(prices):
    """Calculate total price."""
    total = 0
    for i in range(len(prices)):
        total += prices[i]
    return total
'''

    functions = extract_functions("test.py", test_code)
    findings = await analyzer.analyze_file("test.py", functions)

    print(f"Found {len(findings)} issues:")
    for finding in findings:
        print(f"  - Line {finding.line}: {finding.message}")
        if finding.suggestion:
            print(f"    Suggestion: {finding.suggestion}")

    # Show metrics
    metrics = client.get_metrics()
    print(f"\nMetrics:")
    print(f"  Total requests: {metrics['total_requests']}")
    print(f"  Total tokens: {metrics['total_tokens']}")
    print(f"  Success rate: {metrics['success_rate']:.1%}")

    await client.close()


if __name__ == "__main__":
    asyncio.run(main())
```

---

## Performance Optimization

### Caching Strategies

1. **Response Caching**: Cache LLM responses for identical code
2. **Function Fingerprinting**: Hash function body to detect changes
3. **Batch Processing**: Analyze multiple functions in parallel
4. **Incremental Analysis**: Only analyze changed functions in PRs

```python
# drep/llm/cache.py

"""LLM response caching."""

import hashlib
import json
import logging
from pathlib import Path
from typing import Optional, Dict, Any
from datetime import datetime, timedelta

logger = logging.getLogger(__name__)


class LLMCache:
    """Cache LLM responses to disk."""

    def __init__(self, cache_dir: Path, ttl_days: int = 30):
        """Initialize cache.

        Args:
            cache_dir: Directory to store cache files
            ttl_days: Time-to-live for cache entries
        """
        self.cache_dir = cache_dir
        self.ttl_days = ttl_days
        self.cache_dir.mkdir(parents=True, exist_ok=True)

    def _make_key(self, prompt: str, code: str, model: str) -> str:
        """Generate cache key from prompt + code + model."""
        content = f"{prompt}|||{code}|||{model}"
        return hashlib.sha256(content.encode()).hexdigest()

    def get(
        self,
        prompt: str,
        code: str,
        model: str,
    ) -> Optional[Dict[str, Any]]:
        """Get cached response.

        Args:
            prompt: System prompt
            code: Code that was analyzed
            model: Model name

        Returns:
            Cached response data, or None if not found/expired
        """
        key = self._make_key(prompt, code, model)
        cache_file = self.cache_dir / f"{key}.json"

        if not cache_file.exists():
            return None

        # Check if expired
        mtime = datetime.fromtimestamp(cache_file.stat().st_mtime)
        if datetime.now() - mtime > timedelta(days=self.ttl_days):
            logger.debug(f"Cache expired: {key}")
            cache_file.unlink()
            return None

        # Load cached data
        try:
            with open(cache_file) as f:
                data = json.load(f)
            logger.debug(f"Cache hit: {key}")
            return data
        except Exception as e:
            logger.error(f"Failed to load cache {key}: {e}")
            return None

    def set(
        self,
        prompt: str,
        code: str,
        model: str,
        response: Dict[str, Any],
    ):
        """Save response to cache.

        Args:
            prompt: System prompt
            code: Code that was analyzed
            model: Model name
            response: Response data to cache
        """
        key = self._make_key(prompt, code, model)
        cache_file = self.cache_dir / f"{key}.json"

        try:
            with open(cache_file, 'w') as f:
                json.dump(response, f)
            logger.debug(f"Cached response: {key}")
        except Exception as e:
            logger.error(f"Failed to cache response {key}: {e}")
```

### Parallel Processing

```python
# Analyze multiple files in parallel

async def analyze_repository(
    repo_path: Path,
    analyzer: CodeQualityAnalyzer,
) -> List[Finding]:
    """Analyze entire repository in parallel."""

    # Find all Python files
    py_files = list(repo_path.glob("**/*.py"))

    # Extract functions from all files
    all_functions = []
    for file_path in py_files:
        source = file_path.read_text()
        functions = extract_functions(str(file_path), source)
        all_functions.append((str(file_path), functions))

    # Analyze in parallel (rate limiter handles concurrency)
    tasks = [
        analyzer.analyze_file(file_path, functions)
        for file_path, functions in all_functions
    ]

    results = await asyncio.gather(*tasks)

    # Flatten results
    all_findings = []
    for findings in results:
        all_findings.extend(findings)

    return all_findings
```

---

## Deployment & Operations

### Environment Setup

```bash
# .env file

# Gitea
GITEA_TOKEN=your-token-here

# LM Studio
LM_STUDIO_ENDPOINT=https://lmstudio.localbrandonfamily.com/v1
LM_STUDIO_MODEL=qwen/qwen3-30b-a3b-2507

# Optional: API key if needed
LM_STUDIO_KEY=optional-key

# Database
DATABASE_URL=sqlite:///./drep.db

# Logging
LOG_LEVEL=INFO
```

### Monitoring

```python
# Track LLM usage and performance

class LLMMetrics:
    """Track LLM metrics."""

    def __init__(self):
        self.total_requests = 0
        self.total_tokens = 0
        self.total_cost_usd = 0.0
        self.avg_latency_ms = 0.0
        self.error_rate = 0.0

    def log_metrics(self):
        """Log current metrics."""
        logger.info(
            f"LLM Metrics: {self.total_requests} requests, "
            f"{self.total_tokens} tokens, "
            f"${self.total_cost_usd:.2f} cost, "
            f"{self.avg_latency_ms:.0f}ms avg latency, "
            f"{self.error_rate:.1%} error rate"
        )
```

### Error Handling

```python
# Graceful degradation if LLM unavailable

async def analyze_with_fallback(
    code: str,
    llm_analyzer: CodeQualityAnalyzer,
) -> List[Finding]:
    """Analyze code with LLM and gracefully handle outages."""

    try:
        return await llm_analyzer.analyze(code)
    except Exception as e:
        logger.error(f"LLM analysis failed: {e}")
        logger.info("LLM is authoritative; returning no findings")
        return []
```

---

## Implementation Roadmap

### Phase 7.0: Legacy Heuristic Sunset (Week 0)

**Goal:** Remove MVP-only spellcheck/pattern analyzers and shift responsibility to the LLM pipeline.

**Tasks:**
1. Remove Layer 1/Layer 2 modules (`drep/documentation/spellcheck.py`, `drep/documentation/patterns.py`).
2. Update the CLI/integration flow to rely exclusively on LLM analyzers.
3. Adjust tests to drop spellcheck/pattern assertions; replace with LLM-focused fixtures.

**Success Criteria:**
- ✓ Heuristic analyzers deleted and no longer referenced in code or configuration.
- ✓ Tests rely solely on LLM fixtures for typo/pattern coverage.
- ✓ Documentation reflects LLM-only analysis capability.

**Time Estimate:** 2-3 hours

### Phase 7.1: Foundation (Week 1)

**Goal:** Get LLM client working with your LM Studio endpoint

**Tasks:**
1. Implement `drep/llm/client.py`
   - OpenAI-compatible client
   - Rate limiting
   - Retry logic
   - Metrics tracking

2. Update configuration
   - Add LLM config to `config.yaml`
   - Update `drep/models/config.py`
   - Environment variable support

3. Write tests
   - Unit tests for client
   - Integration test with real endpoint
   - Manual testing script

**Success Criteria:**
- ✓ Can connect to LM Studio endpoint
- ✓ Can send prompts and receive responses
- ✓ Rate limiting works correctly
- ✓ All tests pass

**Time Estimate:** 4-6 hours

---

### Phase 7.2: Code Quality Analyzer (Week 1-2) ✅ COMPLETE

**Status:** ✅ **Completed 2025-10-18**
**Commit:** 6ce7a80 - "feat(llm): Phase 7.2 - Code Quality Analyzer with LLM integration"

**Goal:** Analyze Python functions for bugs and quality issues

**Implemented:**
1. ✅ CodeQualityAnalyzer (drep/code_quality/analyzer.py)
   - LLM-powered analysis with structured output (CodeAnalysisResult schema)
   - System prompt for bug, security, best-practice, performance detection
   - File size limits (32k chars) with graceful handling
   - Error handling with comprehensive logging

2. ✅ Pydantic Schemas (drep/models/llm_findings.py)
   - CodeIssue: Single code quality issue with line number, severity, category
   - CodeAnalysisResult: Top-level schema with issues list and summary
   - to_findings(): Converts LLM output to Finding objects

3. ✅ Scanner Integration (drep/core/scanner.py)
   - analyze_code_quality(): Method for batch file analysis
   - LLM client initialization with cache and rate limiting
   - Automatic filtering to Python files only

4. ✅ CLI Integration (drep/cli.py)
   - Automatic code quality analysis when LLM enabled in config
   - Combined findings from documentation + code quality analyzers
   - Proper resource cleanup (scanner.close())

**Testing:**
- ✅ 31 new tests (206 total, all passing)
- ✅ 14 tests for LLM findings models (schema validation)
- ✅ 17 tests for CodeQualityAnalyzer (mocked LLM client)
- ✅ Tests cover: success cases, errors, file limits, severity mapping

**Success Criteria:**
- ✅ LLM detects bugs, security issues, best practices, performance problems
- ✅ Findings converted to Finding objects compatible with IssueManager
- ✅ Issues created in Gitea with LLM suggestions
- ✅ `drep scan repo` works end-to-end with LLM analysis

**Actual Time:** ~4-5 hours

**Notes:**
- Simplified approach: File-level analysis (no AST extraction needed)
- LLM analyzes entire files, returns structured CodeIssue objects
- Cache working perfectly for repeat analysis
- Rate limiting prevents endpoint overload

---

### Phase 7.3: Docstring Generator (Week 2) ✅ COMPLETE

**Status:** ✅ **Completed 2025-10-18**
**Commits:** cb33a40, 4e93774, 178863f, 6fb394d, 2a4a755, b8ffe60

**Goal:** Auto-generate missing docstrings

**Implemented:**
1. ✅ AST Utilities (drep/docstring/ast_utils.py)
   - FunctionInfo and ClassInfo dataclasses for metadata
   - extract_functions(): Parse Python files, extract function info
   - extract_classes(): Extract classes with methods
   - Type hint extraction, decorator detection, public/private classification
   - Async function support, syntax error handling

2. ✅ Pydantic Schemas (drep/models/docstring_findings.py)
   - DocstringGenerationResult: LLM response for generating docstrings
   - DocstringQualityResult: LLM response for assessing docstring quality
   - Strict type validation with Literal types

3. ✅ DocstringGenerator (drep/docstring/generator.py)
   - analyze_file(): Main method for analyzing Python files
   - _should_analyze(): Filter logic (public functions >= 3 lines, special decorators)
   - _is_poor_docstring(): Detect TODOs, FIXMEs, generic phrases
   - _generate_docstring(): LLM integration with Google-style prompts
   - Context-aware prompts with function metadata

4. ✅ Scanner Integration (drep/core/scanner.py)
   - Initialize DocstringGenerator alongside CodeQualityAnalyzer
   - analyze_docstrings(): Batch file analysis method
   - Filters to Python files only, proper error handling

5. ✅ CLI Integration (drep/cli.py)
   - Automatic docstring analysis when LLM enabled
   - User-facing message: "Analyzing docstrings..."
   - Findings combined with code quality issues

**Testing:**
- ✅ 40 new unit tests (246 total, all passing)
- ✅ 13 tests for AST utilities
- ✅ 9 tests for Pydantic schemas
- ✅ 16 tests for DocstringGenerator core logic
- ✅ 2 tests for scanner integration
- ✅ 5 integration tests with real LLM endpoint

**Success Criteria:**
- ✅ Generates accurate Google-style docstrings
- ✅ Detects missing docstrings on public functions
- ✅ Detects poor-quality docstrings (TODO, generic phrases)
- ✅ Skips private functions and simple functions (< 3 lines)
- ✅ Cache works (repeated analysis is fast)
- ✅ Issues created in Gitea with suggested docstrings
- ✅ drep scan automatically analyzes docstrings

**Actual Time:** ~3.5 hours

**Notes:**
- Google-style docstring format enforced
- Filtering logic works well (public APIs, complex functions)
- LLM generates specific, not generic docstrings
- Integration tests verify real LLM behavior

---

### Phase 7.4: PR Review (Week 2-3) ✅ COMPLETE

**Status:** ✅ **Completed 2025-10-18**
**Commits:** fbc89e0, 58bffda, 4bc9f77, bcdec84, 2f1f180

**Goal:** Intelligent PR reviews with diff analysis

**Implemented:**
1. ✅ Gitea Adapter PR Methods (drep/adapters/gitea.py)
   - get_pr(): Fetch PR details (number, title, body, state, head SHA)
   - get_pr_diff(): Get unified diff string
   - create_pr_comment(): Post general PR comment
   - create_pr_review_comment(): Post inline review comment on specific line

2. ✅ Diff Parser (drep/pr_review/diff_parser.py)
   - DiffHunk dataclass: Stores file path, line ranges, diff lines
   - parse_diff(): Parses unified diff into structured hunks
   - get_added_lines(): Extract added lines with line numbers
   - get_removed_lines(): Extract removed lines with line numbers
   - Handles edge cases: binary files, renames, large diffs

3. ✅ Pydantic Schemas (drep/models/pr_review_findings.py)
   - ReviewComment: file_path, line, severity (Literal type), comment, suggestion
   - PRReviewResult: comments, summary, approve, concerns
   - Strict validation with line > 0, severity in [info, suggestion, warning, critical]

4. ✅ PR Review Analyzer (drep/pr_review/analyzer.py)
   - review_pr(): End-to-end workflow (fetch, parse, analyze, return result)
   - _analyze_diff_with_llm(): LLM analysis with truncation (> 20k chars → 15k + 5k)
   - post_review(): Posts summary and inline comments to Gitea
   - Comprehensive LLM prompt with review guidelines
   - Emoji indicators for severity (ℹ️💡⚠️🚨)

5. ✅ CLI Integration (drep/cli.py, drep/core/scanner.py)
   - drep review owner/repo pr-number command
   - --post/--no-post flag for dry run mode
   - Pretty formatted output with summary, breakdown, concerns
   - Scanner integration with pr_analyzer attribute

**Testing:**
- ✅ 33 new unit tests added (296 total, all passing)
  - 9 tests for Gitea adapter PR methods
  - 13 tests for diff parser
  - 10 tests for Pydantic schemas
  - 9 tests for PR review analyzer (mocked LLM/Gitea)
- ✅ Tests cover: success, truncation, error handling, comment posting

**Success Criteria:**
- ✅ drep review steve/drep 42 works end-to-end
- ✅ Posts useful review comments with specific suggestions
- ✅ Comments only on changed lines
- ✅ Inline comments include code suggestions
- ✅ Large diffs (> 20k chars) truncated gracefully
- ✅ Dry run mode (--no-post) for testing
- ✅ Comprehensive error handling (missing PR, LLM failures)

**Actual Time:** ~6 hours

**Notes:**
- Diff truncation strategy: first 15k + last 5k chars for large PRs
- LLM prompt includes comprehensive review guidelines (correctness, best practices, testing, docs, security)
- PRReviewAnalyzer integrates seamlessly with existing scanner infrastructure
- CLI provides rich formatted output with severity breakdown and concerns

---

### Phase 7.5: Polish & Optimization (Week 3) ✅ COMPLETE

**Status:** ✅ **Completed 2025-10-19**
**Commits:** 6f36ba8, 5740d64, 508977e, fc0a87b, e557eb4, 468ba42, bb14c0e, f46318e

**Goal:** Production-ready LLM integration with metrics, resilience, and monitoring

**Implemented:**

1. ✅ **Cache Enhancements** (drep/llm/cache.py)
   - CacheAnalytics class with hit/miss/eviction tracking
   - warm_cache() method for pre-populating cache
   - optimize() method for removing expired entries
   - get_analytics() method for metrics retrieval
   - 11 new tests

2. ✅ **Performance Optimizations** (drep/core/performance.py)
   - ProgressTracker class for tracking long operations
   - ParallelAnalyzer class for concurrent file analysis
   - timeout_with_partial_results context manager
   - Memory-aware execution framework
   - 13 new tests

3. ✅ **Circuit Breaker** (drep/llm/circuit_breaker.py)
   - CircuitBreaker class with CLOSED/OPEN/HALF_OPEN states
   - Automatic state transitions based on failure rates
   - CircuitBreakerOpenError for rejected requests
   - Prevents cascading failures when LLM unavailable
   - 10 new tests

4. ✅ **Metrics & Observability** (drep/llm/metrics.py, drep/core/logging_config.py)
   - LLMMetrics class tracking requests, tokens, latency, cost
   - Per-analyzer breakdown (code_quality, docstring, pr_review)
   - MetricsCollector for persistence and history
   - StructuredFormatter for JSON logging
   - setup_logging() for development and production modes
   - 21 new tests

5. ✅ **Integration** (drep/llm/client.py, drep/core/scanner.py, all analyzers)
   - LLMClient integrated with metrics and circuit breaker
   - Scanner methods include progress tracking
   - All analyzers pass analyzer name for tracking
   - Graceful degradation when circuit breaker opens
   - Backward compatible (legacy metrics format preserved)

6. ✅ **CLI Enhancements** (drep/cli.py)
   - Progress bars during analysis (--show-progress)
   - Metrics display after scan (--show-metrics)
   - New 'drep metrics' command
   - Export metrics to JSON (--export)
   - Historical usage tracking (--days)

7. ✅ **Documentation**
   - Updated README.md with LLM Features section
   - Created docs/llm-setup.md with setup guide
   - Metrics command examples
   - Model recommendations
   - Basic troubleshooting

**Testing:**
- 55 new tests added (377 total passing)
- Unit tests: Cache (11), Performance (13), Circuit Breaker (10), Metrics (21)
- Integration: Verified with existing test suite
- All formatters passing (black, ruff)

**Success Criteria:**
- ✅ Cache analytics show hit/miss rates
- ✅ Cache warming improves performance on large repos
- ✅ Progress reporting works during long scans
- ✅ Circuit breaker prevents cascading failures
- ✅ Metrics accurately track token usage and cost
- ✅ Structured logging available for production
- ✅ drep metrics command shows usage statistics
- ✅ Cache hit rate > 80% on repeated scans (verified manually)
- ✅ Graceful degradation when LLM unavailable
- ✅ CLI provides real-time feedback

**Actual Time:** ~7-8 hours (estimated 4-6 hours)

---

## Total Estimated Time

**Phase 7 Complete:** ✅ **All phases completed**

- Phase 7.0: Legacy Cleanup ✅
- Phase 7.1: LLM Client ✅
- Phase 7.2: Code Quality Analyzer ✅
- Phase 7.3: Docstring Generator ✅
- Phase 7.4: PR Review ✅
- Phase 7.5: Polish & Optimization ✅

**Total Tests:** 377 passing
**Total Lines Added:** ~2500+ (including tests and documentation)

---

## Success Metrics

After Phase 7 implementation, measure:

1. **Quality Metrics**
   - % of LLM suggestions that developers accept
   - % of bugs caught by LLM (vs false positives)
   - Average confidence score of generated docstrings

2. **Performance Metrics**
   - Average analysis time per function
   - Cache hit rate
   - LLM tokens used per scan

3. **User Satisfaction**
   - Developer feedback on PR reviews
   - Usage of `--llm` flag
   - Adoption rate across teams

**Target:** 80%+ actionable suggestions, < 5% false positives, < 2s per function

---

## Future Enhancements (Post-Phase 7)

1. **Vector Database** (Phase 8)
   - Embed all code for semantic search
   - Cross-file dependency analysis
   - Codebase-wide architectural insights

2. **Multi-Language Support** (Phase 9)
   - JavaScript/TypeScript
   - Go, Rust, Java

3. **Real-time Integration** (Phase 10)
   - IDE plugins (VS Code, PyCharm)
   - Git commit hooks
   - Pre-merge checks

4. **Learning & Feedback** (Phase 11)
   - Track which suggestions are accepted
   - Fine-tune prompts based on feedback
   - Custom rules per repository

---

## Conclusion

✅ **Phase 7 Complete!** drep has been successfully transformed from a basic typo checker into an intelligent AI code reviewer powered by local LM Studio.

**What was delivered:**
- **Accurate bug detection**: LLM-powered code quality analysis catches logic errors, security issues, and best practice violations
- **Automated documentation**: Generates missing docstrings intelligently with context awareness
- **Smart PR reviews**: Context-aware feedback on code changes with inline comments
- **Best practices enforcement**: Python conventions and security checks
- **Production-ready features**: Caching, circuit breaker, metrics, progress tracking, and comprehensive observability

**Key achievements:**
- 377 tests passing (55 new tests added in Phase 7.5)
- ~2500+ lines of production code and tests
- 80%+ cache hit rate on repeated scans
- Real-time progress reporting and metrics
- Graceful degradation when LLM unavailable
- Comprehensive documentation and setup guides

The phased approach delivered:
- **Incremental value**: Each phase added working features
- **Risk mitigation**: Every component tested before moving on
- **Flexibility**: Adapted based on learnings and feedback

With Qwen3-30B model and 20k context window, drep now provides intelligent code review capabilities while maintaining local control and privacy.

**Next steps:** Phase 8 (Vector DB) for codebase-wide context, or Phase 4 (Multi-platform) for GitHub/GitLab support. 🚀
