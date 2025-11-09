"""Data classes for the interactive setup wizard.

These frozen dataclasses provide type safety and immutability for configuration
collected during the init wizard, replacing error-prone tuple returns.
"""

from dataclasses import dataclass
from typing import Any, Dict, Optional

# ===== Strongly-Typed Platform Data Models =====


@dataclass(frozen=True)
class GitHubPlatformData:
    """Strongly-typed GitHub platform configuration data.

    Attributes:
        token: GitHub API token (as environment variable reference)
        repositories: Immutable tuple of repository patterns
        url: Optional GitHub Enterprise API URL
    """

    token: str
    repositories: tuple[str, ...]
    url: Optional[str] = None

    def to_dict(self) -> dict[str, Any]:
        """Convert to dictionary for YAML serialization.

        Returns:
            Dict with token, repositories (as list), and optional url
        """
        result = {
            "token": self.token,
            "repositories": list(self.repositories),  # YAML needs lists
        }
        if self.url is not None:
            result["url"] = self.url
        return result


@dataclass(frozen=True)
class GiteaPlatformData:
    """Strongly-typed Gitea platform configuration data.

    Attributes:
        url: Gitea base URL
        token: Gitea API token (as environment variable reference)
        repositories: Immutable tuple of repository patterns
    """

    url: str
    token: str
    repositories: tuple[str, ...]

    def to_dict(self) -> dict[str, Any]:
        """Convert to dictionary for YAML serialization.

        Returns:
            Dict with url, token, and repositories (as list)
        """
        return {
            "url": self.url,
            "token": self.token,
            "repositories": list(self.repositories),
        }


@dataclass(frozen=True)
class GitLabPlatformData:
    """Strongly-typed GitLab platform configuration data.

    Attributes:
        token: GitLab API token (as environment variable reference)
        repositories: Immutable tuple of repository patterns
        url: Optional self-hosted GitLab URL
    """

    token: str
    repositories: tuple[str, ...]
    url: Optional[str] = None

    def to_dict(self) -> dict[str, Any]:
        """Convert to dictionary for YAML serialization.

        Returns:
            Dict with token, repositories (as list), and optional url
        """
        result = {
            "token": self.token,
            "repositories": list(self.repositories),
        }
        if self.url is not None:
            result["url"] = self.url
        return result


# ===== Strongly-Typed LLM Data Models =====


@dataclass(frozen=True)
class BedrockRegionModel:
    """AWS Bedrock region and model configuration.

    Attributes:
        region: AWS region (e.g., "us-east-1")
        model: Bedrock model ID (e.g., "anthropic.claude-sonnet-4-5-20250929-v1:0")
    """

    region: str
    model: str

    def to_dict(self) -> dict[str, str]:
        """Convert to dictionary for YAML serialization.

        Returns:
            Dict with region and model keys
        """
        return {"region": self.region, "model": self.model}


@dataclass(frozen=True)
class OpenAILLMData:
    """Strongly-typed OpenAI-compatible LLM configuration data.

    Attributes:
        enabled: Whether LLM integration is enabled
        provider: Provider name (always "openai-compatible")
        endpoint: API endpoint URL
        model: Model name
        api_key: Optional API key (as environment variable reference)
        temperature: Optional temperature setting
        max_tokens: Optional max tokens per request
        timeout: Optional request timeout
        max_retries: Optional max retry attempts
        retry_delay: Optional retry delay in seconds
        exponential_backoff: Optional exponential backoff flag
        max_concurrent_global: Optional max concurrent requests globally
        max_concurrent_per_repo: Optional max concurrent requests per repo
        requests_per_minute: Optional requests per minute limit
        max_tokens_per_minute: Optional tokens per minute limit
        cache: Optional cache configuration dict
    """

    enabled: bool
    provider: str
    endpoint: str
    model: str
    api_key: Optional[str] = None
    temperature: Optional[float] = None
    max_tokens: Optional[int] = None
    timeout: Optional[int] = None
    max_retries: Optional[int] = None
    retry_delay: Optional[int] = None
    exponential_backoff: Optional[bool] = None
    max_concurrent_global: Optional[int] = None
    max_concurrent_per_repo: Optional[int] = None
    requests_per_minute: Optional[int] = None
    max_tokens_per_minute: Optional[int] = None
    cache: Optional[dict[str, Any]] = None

    def to_dict(self) -> dict[str, Any]:
        """Convert to dictionary for YAML serialization.

        Returns:
            Dict with all non-None fields
        """
        result = {
            "enabled": self.enabled,
            "provider": self.provider,
            "endpoint": self.endpoint,
            "model": self.model,
        }
        # Add optional fields only if set
        if self.api_key is not None:
            result["api_key"] = self.api_key
        if self.temperature is not None:
            result["temperature"] = self.temperature
        if self.max_tokens is not None:
            result["max_tokens"] = self.max_tokens
        if self.timeout is not None:
            result["timeout"] = self.timeout
        if self.max_retries is not None:
            result["max_retries"] = self.max_retries
        if self.retry_delay is not None:
            result["retry_delay"] = self.retry_delay
        if self.exponential_backoff is not None:
            result["exponential_backoff"] = self.exponential_backoff
        if self.max_concurrent_global is not None:
            result["max_concurrent_global"] = self.max_concurrent_global
        if self.max_concurrent_per_repo is not None:
            result["max_concurrent_per_repo"] = self.max_concurrent_per_repo
        if self.requests_per_minute is not None:
            result["requests_per_minute"] = self.requests_per_minute
        if self.max_tokens_per_minute is not None:
            result["max_tokens_per_minute"] = self.max_tokens_per_minute
        if self.cache is not None:
            result["cache"] = self.cache
        return result


@dataclass(frozen=True)
class BedrockLLMData:
    """Strongly-typed AWS Bedrock LLM configuration data.

    Attributes:
        enabled: Whether LLM integration is enabled
        provider: Provider name (always "bedrock")
        bedrock: Bedrock configuration (region and model)
        temperature: Optional temperature setting
        max_tokens: Optional max tokens per request
        timeout: Optional request timeout
        max_retries: Optional max retry attempts
        retry_delay: Optional retry delay in seconds
        exponential_backoff: Optional exponential backoff flag
        max_concurrent_global: Optional max concurrent requests globally
        max_concurrent_per_repo: Optional max concurrent requests per repo
        requests_per_minute: Optional requests per minute limit
        max_tokens_per_minute: Optional tokens per minute limit
        cache: Optional cache configuration dict
    """

    enabled: bool
    provider: str
    bedrock: BedrockRegionModel
    temperature: Optional[float] = None
    max_tokens: Optional[int] = None
    timeout: Optional[int] = None
    max_retries: Optional[int] = None
    retry_delay: Optional[int] = None
    exponential_backoff: Optional[bool] = None
    max_concurrent_global: Optional[int] = None
    max_concurrent_per_repo: Optional[int] = None
    requests_per_minute: Optional[int] = None
    max_tokens_per_minute: Optional[int] = None
    cache: Optional[dict[str, Any]] = None

    def to_dict(self) -> dict[str, Any]:
        """Convert to dictionary for YAML serialization.

        Returns:
            Dict with all non-None fields
        """
        result = {
            "enabled": self.enabled,
            "provider": self.provider,
            "bedrock": self.bedrock.to_dict(),
        }
        # Add optional fields only if set
        if self.temperature is not None:
            result["temperature"] = self.temperature
        if self.max_tokens is not None:
            result["max_tokens"] = self.max_tokens
        if self.timeout is not None:
            result["timeout"] = self.timeout
        if self.max_retries is not None:
            result["max_retries"] = self.max_retries
        if self.retry_delay is not None:
            result["retry_delay"] = self.retry_delay
        if self.exponential_backoff is not None:
            result["exponential_backoff"] = self.exponential_backoff
        if self.max_concurrent_global is not None:
            result["max_concurrent_global"] = self.max_concurrent_global
        if self.max_concurrent_per_repo is not None:
            result["max_concurrent_per_repo"] = self.max_concurrent_per_repo
        if self.requests_per_minute is not None:
            result["requests_per_minute"] = self.requests_per_minute
        if self.max_tokens_per_minute is not None:
            result["max_tokens_per_minute"] = self.max_tokens_per_minute
        if self.cache is not None:
            result["cache"] = self.cache
        return result


@dataclass(frozen=True)
class AnthropicLLMData:
    """Strongly-typed Anthropic LLM configuration data.

    Attributes:
        enabled: Whether LLM integration is enabled
        provider: Provider name (always "anthropic")
        api_key: Anthropic API key (as environment variable reference)
        model: Model name
        temperature: Optional temperature setting
        max_tokens: Optional max tokens per request
        timeout: Optional request timeout
        max_retries: Optional max retry attempts
        retry_delay: Optional retry delay in seconds
        exponential_backoff: Optional exponential backoff flag
        max_concurrent_global: Optional max concurrent requests globally
        max_concurrent_per_repo: Optional max concurrent requests per repo
        requests_per_minute: Optional requests per minute limit
        max_tokens_per_minute: Optional tokens per minute limit
        cache: Optional cache configuration dict
    """

    enabled: bool
    provider: str
    api_key: str
    model: str
    temperature: Optional[float] = None
    max_tokens: Optional[int] = None
    timeout: Optional[int] = None
    max_retries: Optional[int] = None
    retry_delay: Optional[int] = None
    exponential_backoff: Optional[bool] = None
    max_concurrent_global: Optional[int] = None
    max_concurrent_per_repo: Optional[int] = None
    requests_per_minute: Optional[int] = None
    max_tokens_per_minute: Optional[int] = None
    cache: Optional[dict[str, Any]] = None

    def to_dict(self) -> dict[str, Any]:
        """Convert to dictionary for YAML serialization.

        Returns:
            Dict with all non-None fields
        """
        result = {
            "enabled": self.enabled,
            "provider": self.provider,
            "api_key": self.api_key,
            "model": self.model,
        }
        # Add optional fields only if set
        if self.temperature is not None:
            result["temperature"] = self.temperature
        if self.max_tokens is not None:
            result["max_tokens"] = self.max_tokens
        if self.timeout is not None:
            result["timeout"] = self.timeout
        if self.max_retries is not None:
            result["max_retries"] = self.max_retries
        if self.retry_delay is not None:
            result["retry_delay"] = self.retry_delay
        if self.exponential_backoff is not None:
            result["exponential_backoff"] = self.exponential_backoff
        if self.max_concurrent_global is not None:
            result["max_concurrent_global"] = self.max_concurrent_global
        if self.max_concurrent_per_repo is not None:
            result["max_concurrent_per_repo"] = self.max_concurrent_per_repo
        if self.requests_per_minute is not None:
            result["requests_per_minute"] = self.requests_per_minute
        if self.max_tokens_per_minute is not None:
            result["max_tokens_per_minute"] = self.max_tokens_per_minute
        if self.cache is not None:
            result["cache"] = self.cache
        return result


# ===== Strongly-Typed Documentation Data Model =====


@dataclass(frozen=True)
class DocumentationConfigData:
    """Strongly-typed documentation configuration data.

    Attributes:
        enabled: Whether documentation analysis is enabled
        markdown_checks: Whether markdown lint checks are enabled
        custom_dictionary: Immutable tuple of custom dictionary words
    """

    enabled: bool
    markdown_checks: bool = False
    custom_dictionary: tuple[str, ...] = ()

    def to_dict(self) -> dict[str, Any]:
        """Convert to dictionary for YAML serialization.

        Returns:
            Dict with enabled, markdown_checks, and custom_dictionary (as list)
        """
        return {
            "enabled": self.enabled,
            "markdown_checks": self.markdown_checks,
            "custom_dictionary": list(self.custom_dictionary),
        }


# ===== Wrapper Classes (Updated for Strongly-Typed Data) =====


@dataclass(frozen=True)
class PlatformConfig:
    """Platform configuration collected from init wizard.

    Supports both old Dict[str, Any] format (for backward compatibility)
    and new strongly-typed format using platform-specific data models.

    Attributes:
        config: DEPRECATED - Platform configuration dict (for backward compatibility)
        data: Strongly-typed platform data (GitHubPlatformData, GiteaPlatformData,
              or GitLabPlatformData)
        env_var: Required environment variable name (e.g., "GITHUB_TOKEN")
        platform_name: Human-readable platform name (e.g., "GitHub")
    """

    env_var: str
    platform_name: str
    config: Optional[Dict[str, Any]] = None  # Deprecated, for backward compatibility
    data: Optional[GitHubPlatformData | GiteaPlatformData | GitLabPlatformData] = None

    def __post_init__(self) -> None:
        """Validate that exactly one of config or data is provided.

        Raises:
            ValueError: If both or neither config and data are provided
        """
        # Must have exactly one of config or data
        if self.config is None and self.data is None:
            raise ValueError("Must provide either 'config' or 'data'")
        if self.config is not None and self.data is not None:
            raise ValueError("Cannot provide both 'config' and 'data'")

        # If using old config format, validate token field
        if self.config is not None:
            platform_key = self.platform_name.lower()
            platform_dict = self.config.get(platform_key, {})
            if "token" not in platform_dict:
                raise ValueError(
                    f"Platform config for {self.platform_name} must include 'token' field"
                )

    def to_dict(self) -> dict[str, Any]:
        """Convert to dictionary for YAML serialization.

        Returns:
            Dict in format: {"github": {...}} or {"gitea": {...}} or {"gitlab": {...}}
        """
        if self.data is not None:
            # New strongly-typed format
            platform_key = self.platform_name.lower()
            return {platform_key: self.data.to_dict()}
        else:
            # Old dict format - return as-is
            return self.config  # type: ignore


@dataclass(frozen=True)
class LLMConfig:
    """LLM configuration collected from init wizard.

    Supports both old Dict[str, Any] format (for backward compatibility)
    and new strongly-typed format using provider-specific data models.

    Attributes:
        config: DEPRECATED - LLM configuration dict (for backward compatibility)
        data: Strongly-typed LLM data (OpenAILLMData, BedrockLLMData, or AnthropicLLMData)
        provider: LLM provider name (deprecated, derived from data if using new format)
    """

    provider: str = ""  # Deprecated, will be derived from data if using new format
    config: Optional[Dict[str, Any]] = None  # Deprecated, for backward compatibility
    data: Optional[OpenAILLMData | BedrockLLMData | AnthropicLLMData] = None

    def __post_init__(self) -> None:
        """Validate that exactly one of config or data is provided.

        Raises:
            ValueError: If both or neither config and data are provided
        """
        # Must have exactly one of config or data
        if self.config is None and self.data is None:
            raise ValueError("Must provide either 'config' or 'data'")
        if self.config is not None and self.data is not None:
            raise ValueError("Cannot provide both 'config' and 'data'")

        # If using old config format, validate required fields
        if self.config is not None:
            llm_dict = self.config.get("llm", {})
            if "enabled" not in llm_dict:
                raise ValueError("LLM config must include 'enabled' field")
            if "provider" not in llm_dict:
                raise ValueError("LLM config must include 'provider' field")

        # If using new format, override provider with data's provider
        if self.data is not None:
            # This works via object.__setattr__ since frozen=True
            object.__setattr__(self, "provider", self.data.provider)

    def to_dict(self) -> dict[str, Any]:
        """Convert to dictionary for YAML serialization.

        Returns:
            Dict in format: {"llm": {...}}
        """
        if self.data is not None:
            # New strongly-typed format
            return {"llm": self.data.to_dict()}
        else:
            # Old dict format - return as-is
            return self.config  # type: ignore


@dataclass(frozen=True)
class DocumentationConfig:
    """Documentation analysis configuration.

    Supports both old Dict[str, Any] format (for backward compatibility)
    and new strongly-typed format using DocumentationConfigData.

    Attributes:
        config: DEPRECATED - Documentation configuration dict (for backward compatibility)
        data: Strongly-typed documentation data
    """

    config: Optional[Dict[str, Any]] = None  # Deprecated, for backward compatibility
    data: Optional[DocumentationConfigData] = None

    def __post_init__(self) -> None:
        """Validate that exactly one of config or data is provided.

        Raises:
            ValueError: If both or neither config and data are provided
        """
        # Must have exactly one of config or data
        if self.config is None and self.data is None:
            raise ValueError("Must provide either 'config' or 'data'")
        if self.config is not None and self.data is not None:
            raise ValueError("Cannot provide both 'config' and 'data'")

        # If using old config format, validate required fields
        if self.config is not None:
            doc_dict = self.config.get("documentation", {})
            if "enabled" not in doc_dict:
                raise ValueError("Documentation config must include 'enabled' field")

    def to_dict(self) -> dict[str, Any]:
        """Convert to dictionary for YAML serialization.

        Returns:
            Dict in format: {"documentation": {...}}
        """
        if self.data is not None:
            # New strongly-typed format
            return {"documentation": self.data.to_dict()}
        else:
            # Old dict format - return as-is
            return self.config  # type: ignore
