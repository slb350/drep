"""Configuration models for drep."""

from pathlib import Path
from typing import List, Optional

from pydantic import BaseModel, Field, HttpUrl


class GiteaConfig(BaseModel):
    """Gitea platform configuration."""

    url: str = Field(..., description="Gitea base URL (e.g., http://192.168.1.14:3000)")
    token: str = Field(..., description="Gitea API token")
    repositories: List[str] = Field(..., description="Repository patterns (e.g., steve/*)")


class DocumentationConfig(BaseModel):
    """Documentation analysis settings."""

    enabled: bool = True
    custom_dictionary: List[str] = Field(default_factory=list)
    markdown_checks: bool = Field(
        default=False,
        description="Enable basic Markdown lint checks (headings, trailing whitespace, code fences)",
    )


class CacheConfig(BaseModel):
    """LLM response cache configuration."""

    enabled: bool = Field(default=True, description="Enable response caching")
    directory: Path = Field(
        default=Path.home() / ".cache" / "drep" / "llm",
        description="Cache directory path",
    )
    ttl_days: int = Field(default=30, ge=1, description="Time-to-live in days for cached responses")
    max_size_gb: float = Field(default=10.0, ge=0.1, description="Maximum cache size in gigabytes")


class LLMConfig(BaseModel):
    """LLM client configuration."""

    enabled: bool = Field(default=False, description="Enable LLM-powered analysis")
    endpoint: HttpUrl = Field(..., description="OpenAI-compatible API endpoint")
    model: str = Field(..., description="Model name to use")
    api_key: Optional[str] = Field(
        default=None, description="API key (optional for local endpoints)"
    )
    temperature: float = Field(default=0.2, ge=0.0, le=2.0, description="Sampling temperature")
    max_tokens: int = Field(
        default=8000, ge=100, le=20000, description="Maximum tokens per request"
    )
    timeout: int = Field(default=60, ge=10, le=300, description="Request timeout in seconds")
    max_retries: int = Field(
        default=3, ge=0, le=10, description="Maximum number of retries on failure"
    )
    retry_delay: int = Field(default=2, ge=1, le=60, description="Initial retry delay in seconds")
    exponential_backoff: bool = Field(
        default=True, description="Use exponential backoff for retries"
    )
    max_concurrent_global: int = Field(
        default=5, ge=1, le=50, description="Maximum concurrent requests globally"
    )
    max_concurrent_per_repo: int = Field(
        default=3, ge=1, le=20, description="Maximum concurrent requests per repository"
    )
    requests_per_minute: int = Field(
        default=60, ge=1, le=1000, description="Rate limit: requests per minute"
    )
    max_tokens_per_minute: int = Field(
        default=100000, ge=1000, description="Rate limit: tokens per minute"
    )
    cache: CacheConfig = Field(default_factory=CacheConfig, description="Cache settings")


class Config(BaseModel):
    """Main configuration."""

    gitea: GiteaConfig
    documentation: DocumentationConfig
    database_url: str = "sqlite:///./drep.db"
    llm: Optional[LLMConfig] = Field(default=None, description="LLM configuration")
