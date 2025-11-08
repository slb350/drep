"""Configuration models for drep."""

from pathlib import Path
from typing import List, Optional

from pydantic import BaseModel, Field, HttpUrl, SecretStr, field_validator, model_validator


class GiteaConfig(BaseModel):
    """Gitea platform configuration."""

    url: str = Field(..., description="Gitea base URL (e.g., http://192.168.1.14:3000)")
    token: SecretStr = Field(..., description="Gitea API token")
    repositories: List[str] = Field(..., description="Repository patterns (e.g., steve/*)")


class GitHubConfig(BaseModel):
    """GitHub platform configuration."""

    token: SecretStr = Field(
        ..., description="GitHub Personal Access Token (PAT) or GitHub App token"
    )
    repositories: List[str] = Field(
        ..., description="Repository patterns (e.g., owner/repo or owner/*)"
    )
    url: HttpUrl = Field(
        default="https://api.github.com",
        description=(
            "GitHub API URL (default: https://api.github.com, " "use custom for GitHub Enterprise)"
        ),
    )


class DocumentationConfig(BaseModel):
    """Documentation analysis settings."""

    enabled: bool = True
    custom_dictionary: List[str] = Field(default_factory=list)
    markdown_checks: bool = Field(
        default=False,
        description=(
            "Enable basic Markdown lint checks " "(headings, trailing whitespace, code fences)"
        ),
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


class BedrockConfig(BaseModel):
    """AWS Bedrock configuration."""

    region: str = Field(
        default="us-east-1",
        description="AWS region for Bedrock (e.g., us-east-1, us-west-2)",
    )
    model: str = Field(
        default="anthropic.claude-sonnet-4-5-20250929-v1:0",
        description="Bedrock model ID (e.g., anthropic.claude-sonnet-4-5-20250929-v1:0)",
    )

    @field_validator("model")
    @classmethod
    def validate_model_id(cls, v: str) -> str:
        """Validate Bedrock model ID format.

        Ensures model ID starts with a valid provider prefix.
        """
        valid_prefixes = [
            "anthropic.",
            "global.anthropic.",
            "amazon.",
            "global.amazon.",
            "meta.",
            "global.meta.",
            "cohere.",
            "global.cohere.",
        ]
        if not any(v.startswith(prefix) for prefix in valid_prefixes):
            raise ValueError(
                f"Invalid Bedrock model ID: {v}. "
                f"Must start with a valid provider prefix: {', '.join(valid_prefixes)}"
            )
        return v


class LLMConfig(BaseModel):
    """LLM client configuration."""

    enabled: bool = Field(default=False, description="Enable LLM-powered analysis")
    provider: str = Field(
        default="openai-compatible",
        description="LLM provider: openai-compatible, bedrock, anthropic",
    )
    endpoint: Optional[HttpUrl] = Field(
        default=None,
        description="OpenAI-compatible API endpoint (required for openai-compatible provider)",
    )
    model: Optional[str] = Field(
        default=None, description="Model name to use (required for openai-compatible provider)"
    )
    bedrock: Optional[BedrockConfig] = Field(
        default=None, description="AWS Bedrock configuration (required if provider=bedrock)"
    )
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

    @model_validator(mode="after")
    def validate_bedrock_config(self) -> "LLMConfig":
        """Ensure Bedrock config is provided when provider is bedrock.

        Raises:
            ValueError: If provider is bedrock but bedrock config is missing
        """
        if self.provider == "bedrock" and self.bedrock is None:
            raise ValueError(
                "Bedrock provider requires 'bedrock' configuration with region and model. "
                "Please add 'bedrock:' section to your config."
            )
        return self

    @model_validator(mode="after")
    def validate_openai_config(self) -> "LLMConfig":
        """Ensure endpoint and model are provided for openai-compatible provider.

        Raises:
            ValueError: If provider is openai-compatible but endpoint or model is missing
        """
        if self.provider == "openai-compatible":
            if self.endpoint is None:
                raise ValueError(
                    "OpenAI-compatible provider requires 'endpoint' field. "
                    "Please specify the API endpoint URL."
                )
            if self.model is None:
                raise ValueError(
                    "OpenAI-compatible provider requires 'model' field. "
                    "Please specify the model name to use."
                )
        return self


class Config(BaseModel):
    """Main configuration.

    At least one platform (gitea or github) must be configured.
    """

    gitea: Optional[GiteaConfig] = Field(
        default=None, description="Gitea platform configuration (optional)"
    )
    github: Optional[GitHubConfig] = Field(
        default=None, description="GitHub platform configuration (optional)"
    )
    documentation: DocumentationConfig = Field(default_factory=DocumentationConfig)
    database_url: str = "sqlite:///./drep.db"
    llm: Optional[LLMConfig] = Field(default=None, description="LLM configuration")

    # Internal field to control platform validation (for pre-commit hooks)
    # Excluded from serialization but accessible in validators
    require_platform_config: bool = Field(
        default=True, exclude=True, description="Internal flag for platform validation"
    )

    @model_validator(mode="after")
    def validate_at_least_one_platform(self) -> "Config":
        """Ensure at least one platform is configured.

        Raises:
            ValueError: If neither gitea nor github is configured (when required)

        Note:
            Platform validation is skipped when require_platform_config=False
            (used for pre-commit hooks with local-only analysis)
        """
        # Skip validation if platform not required (pre-commit mode)
        if not self.require_platform_config:
            return self

        # Validate platform presence
        if self.gitea is None and self.github is None:
            raise ValueError(
                "At least one platform must be configured. "
                "Please provide 'gitea' or 'github' configuration."
            )
        return self
