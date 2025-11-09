"""Data classes for the interactive setup wizard.

These frozen dataclasses provide type safety and immutability for configuration
collected during the init wizard, replacing error-prone tuple returns.
"""

from dataclasses import dataclass
from typing import Any, Dict, Optional


@dataclass(frozen=True)
class PlatformConfig:
    """Platform configuration collected from init wizard.

    Attributes:
        config: Platform configuration dict (e.g., {"github": {"token": "...", ...}})
        env_var: Required environment variable name (e.g., "GITHUB_TOKEN")
        platform_name: Human-readable platform name (e.g., "GitHub")
    """

    config: Dict[str, Any]
    env_var: str
    platform_name: str

    def __post_init__(self) -> None:
        """Validate that platform config includes required token field.

        Raises:
            ValueError: If platform config is missing 'token' field
        """
        # Get the platform-specific config dict (config is {"platform": {...}})
        platform_key = self.platform_name.lower()
        platform_dict = self.config.get(platform_key, {})

        if "token" not in platform_dict:
            raise ValueError(
                f"Platform config for {self.platform_name} must include 'token' field"
            )


@dataclass(frozen=True)
class LLMConfig:
    """LLM configuration collected from init wizard.

    Attributes:
        config: LLM configuration dict (e.g., {"llm": {"provider": "...", ...}})
        provider: LLM provider name (e.g., "openai-compatible", "bedrock", "anthropic")
    """

    config: Dict[str, Any]
    provider: str

    def __post_init__(self) -> None:
        """Validate that LLM config includes required fields.

        Raises:
            ValueError: If LLM config is missing required fields
        """
        llm_dict = self.config.get("llm", {})

        if "enabled" not in llm_dict:
            raise ValueError("LLM config must include 'enabled' field")

        if "provider" not in llm_dict:
            raise ValueError("LLM config must include 'provider' field")


@dataclass(frozen=True)
class DocumentationConfig:
    """Documentation analysis configuration.

    Attributes:
        config: Documentation configuration dict (e.g., {"documentation": {"enabled": True, ...}})
    """

    config: Dict[str, Any]

    def __post_init__(self) -> None:
        """Validate that documentation config includes required fields.

        Raises:
            ValueError: If documentation config is missing required fields
        """
        doc_dict = self.config.get("documentation", {})

        if "enabled" not in doc_dict:
            raise ValueError("Documentation config must include 'enabled' field")
