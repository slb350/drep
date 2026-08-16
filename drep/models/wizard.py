"""Data classes for the interactive setup wizard.

These frozen dataclasses provide type safety and immutability for configuration
collected during the init wizard, replacing error-prone tuple returns.
"""

from dataclasses import dataclass
from typing import Any, ClassVar

# ===== Strongly-Typed Platform Data Models =====
#
# Each variant carries its own identity (config_key / display_name /
# token_env_var) as class constants. Adding a platform means adding one
# dataclass, not editing a config-key map, a display-name map, a token
# env-var map, and the wizard call sites that hand-pass them.


@dataclass(frozen=True)
class GitHubPlatformData:
    """Strongly-typed GitHub platform configuration data.

    Attributes:
        token: GitHub API token (as environment variable reference)
        repositories: Immutable tuple of repository patterns
        url: Optional GitHub Enterprise API URL
    """

    config_key: ClassVar[str] = "github"
    display_name: ClassVar[str] = "GitHub"
    token_env_var: ClassVar[str] = "GITHUB_TOKEN"

    token: str
    repositories: tuple[str, ...]
    url: str | None = None

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

    config_key: ClassVar[str] = "gitea"
    display_name: ClassVar[str] = "Gitea"
    token_env_var: ClassVar[str] = "GITEA_TOKEN"

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

    config_key: ClassVar[str] = "gitlab"
    display_name: ClassVar[str] = "GitLab"
    token_env_var: ClassVar[str] = "GITLAB_TOKEN"

    token: str
    repositories: tuple[str, ...]
    url: str | None = None

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
    api_key: str | None = None
    temperature: float | None = None
    max_tokens: int | None = None
    timeout: int | None = None
    max_retries: int | None = None
    retry_delay: int | None = None
    exponential_backoff: bool | None = None
    max_concurrent_global: int | None = None
    max_concurrent_per_repo: int | None = None
    requests_per_minute: int | None = None
    max_tokens_per_minute: int | None = None
    cache: dict[str, Any] | None = None

    def required_env_vars(self) -> tuple[str, ...]:
        """Environment variables the user must set for this LLM provider.

        Derived from the typed data rather than searching the serialized YAML
        for a "${LLM_API_KEY}" substring, so it cannot fall out of step with
        what the wizard actually wrote.
        """
        return ("LLM_API_KEY",) if self.api_key else ()

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
    temperature: float | None = None
    max_tokens: int | None = None
    timeout: int | None = None
    max_retries: int | None = None
    retry_delay: int | None = None
    exponential_backoff: bool | None = None
    max_concurrent_global: int | None = None
    max_concurrent_per_repo: int | None = None
    requests_per_minute: int | None = None
    max_tokens_per_minute: int | None = None
    cache: dict[str, Any] | None = None

    def required_env_vars(self) -> tuple[str, ...]:
        """Environment variables the user must set for this LLM provider."""
        return ("AWS_ACCESS_KEY_ID", "AWS_SECRET_ACCESS_KEY")

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


# ===== Wrapper Classes =====


@dataclass(frozen=True)
class PlatformConfig:
    """Platform configuration collected from init wizard.

    The YAML key, display name, token environment variable, and serialized
    payload are all derived from the concrete ``data`` variant — there is no
    parallel string field that can disagree with the data.

    Attributes:
        data: Strongly-typed platform data (GitHubPlatformData, GiteaPlatformData,
              or GitLabPlatformData)
    """

    data: GitHubPlatformData | GiteaPlatformData | GitLabPlatformData

    @property
    def platform_name(self) -> str:
        """Human-readable platform name derived from the data variant."""
        return self.data.display_name

    @property
    def env_var(self) -> str:
        """Environment variable holding this platform's API token."""
        return self.data.token_env_var

    def to_dict(self) -> dict[str, Any]:
        """Convert to dictionary for YAML serialization.

        Returns:
            Dict in format: {"github": {...}} or {"gitea": {...}} or {"gitlab": {...}}
        """
        return {self.data.config_key: self.data.to_dict()}


@dataclass(frozen=True)
class LLMConfig:
    """LLM configuration collected from init wizard.

    Attributes:
        data: Strongly-typed LLM data (OpenAILLMData or BedrockLLMData)
    """

    data: OpenAILLMData | BedrockLLMData

    @property
    def provider(self) -> str:
        """Provider name, derived from the data variant."""
        return self.data.provider

    def to_dict(self) -> dict[str, Any]:
        """Convert to dictionary for YAML serialization.

        Returns:
            Dict in format: {"llm": {...}}
        """
        return {"llm": self.data.to_dict()}


@dataclass(frozen=True)
class DocumentationConfig:
    """Documentation analysis configuration.

    Attributes:
        data: Strongly-typed documentation data
    """

    data: DocumentationConfigData

    def to_dict(self) -> dict[str, Any]:
        """Convert to dictionary for YAML serialization.

        Returns:
            Dict in format: {"documentation": {...}}
        """
        return {"documentation": self.data.to_dict()}


#: Platform config key -> token environment variable, derived from the data
#: variants so it cannot disagree with them.
PLATFORM_TOKEN_ENV_VARS: dict[str, str] = {
    variant.config_key: variant.token_env_var
    for variant in (GitHubPlatformData, GiteaPlatformData, GitLabPlatformData)
}
