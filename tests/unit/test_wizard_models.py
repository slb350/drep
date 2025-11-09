"""Tests for wizard data models."""

import pytest

from drep.models.wizard import DocumentationConfig, LLMConfig, PlatformConfig


class TestPlatformConfig:
    """Tests for PlatformConfig dataclass."""

    def test_valid_github_config(self):
        """Test valid GitHub platform configuration."""
        config = PlatformConfig(
            config={"github": {"token": "${GITHUB_TOKEN}", "repositories": ["owner/*"]}},
            env_var="GITHUB_TOKEN",
            platform_name="GitHub",
        )
        assert config.config == {
            "github": {"token": "${GITHUB_TOKEN}", "repositories": ["owner/*"]}
        }
        assert config.env_var == "GITHUB_TOKEN"
        assert config.platform_name == "GitHub"

    def test_valid_gitea_config(self):
        """Test valid Gitea platform configuration."""
        config = PlatformConfig(
            config={
                "gitea": {
                    "url": "http://localhost:3000",
                    "token": "${GITEA_TOKEN}",
                    "repositories": ["org/*"],
                }
            },
            env_var="GITEA_TOKEN",
            platform_name="Gitea",
        )
        assert config.platform_name == "Gitea"
        assert config.env_var == "GITEA_TOKEN"

    def test_valid_gitlab_config(self):
        """Test valid GitLab platform configuration."""
        config = PlatformConfig(
            config={"gitlab": {"token": "${GITLAB_TOKEN}", "repositories": ["group/*"]}},
            env_var="GITLAB_TOKEN",
            platform_name="GitLab",
        )
        assert config.platform_name == "GitLab"
        assert config.env_var == "GITLAB_TOKEN"

    def test_missing_token_raises_error(self):
        """Test that missing token field raises ValueError."""
        with pytest.raises(ValueError, match="must include 'token' field"):
            PlatformConfig(
                config={"github": {"repositories": ["owner/*"]}},  # Missing token
                env_var="GITHUB_TOKEN",
                platform_name="GitHub",
            )

    def test_frozen_dataclass(self):
        """Test that PlatformConfig is immutable."""
        config = PlatformConfig(
            config={"github": {"token": "${GITHUB_TOKEN}"}},
            env_var="GITHUB_TOKEN",
            platform_name="GitHub",
        )
        with pytest.raises(AttributeError):
            config.env_var = "MODIFIED"  # type: ignore


class TestLLMConfig:
    """Tests for LLMConfig dataclass."""

    def test_valid_openai_compatible_config(self):
        """Test valid OpenAI-compatible LLM configuration."""
        config = LLMConfig(
            config={
                "llm": {
                    "enabled": True,
                    "provider": "openai-compatible",
                    "endpoint": "http://localhost:1234/v1",
                    "model": "qwen3-30b-a3b",
                }
            },
            provider="openai-compatible",
        )
        assert config.provider == "openai-compatible"
        assert config.config["llm"]["enabled"] is True

    def test_valid_bedrock_config(self):
        """Test valid AWS Bedrock LLM configuration."""
        config = LLMConfig(
            config={
                "llm": {
                    "enabled": True,
                    "provider": "bedrock",
                    "bedrock": {"region": "us-east-1", "model": "anthropic.claude-v2"},
                }
            },
            provider="bedrock",
        )
        assert config.provider == "bedrock"

    def test_valid_anthropic_config(self):
        """Test valid Anthropic LLM configuration."""
        config = LLMConfig(
            config={
                "llm": {
                    "enabled": True,
                    "provider": "anthropic",
                    "api_key": "${ANTHROPIC_API_KEY}",
                    "model": "claude-sonnet-4-5-20250929",
                }
            },
            provider="anthropic",
        )
        assert config.provider == "anthropic"

    def test_missing_enabled_raises_error(self):
        """Test that missing enabled field raises ValueError."""
        with pytest.raises(ValueError, match="must include 'enabled' field"):
            LLMConfig(
                config={"llm": {"provider": "openai-compatible"}},  # Missing enabled
                provider="openai-compatible",
            )

    def test_missing_provider_raises_error(self):
        """Test that missing provider field raises ValueError."""
        with pytest.raises(ValueError, match="must include 'provider' field"):
            LLMConfig(
                config={"llm": {"enabled": True}},  # Missing provider
                provider="openai-compatible",
            )

    def test_frozen_dataclass(self):
        """Test that LLMConfig is immutable."""
        config = LLMConfig(
            config={"llm": {"enabled": True, "provider": "anthropic"}},
            provider="anthropic",
        )
        with pytest.raises(AttributeError):
            config.provider = "bedrock"  # type: ignore


class TestDocumentationConfig:
    """Tests for DocumentationConfig dataclass."""

    def test_valid_enabled_config(self):
        """Test valid documentation configuration with enabled=True."""
        config = DocumentationConfig(
            config={
                "documentation": {"enabled": True, "markdown_checks": True, "custom_dictionary": []}
            }
        )
        assert config.config["documentation"]["enabled"] is True

    def test_valid_disabled_config(self):
        """Test valid documentation configuration with enabled=False."""
        config = DocumentationConfig(config={"documentation": {"enabled": False}})
        assert config.config["documentation"]["enabled"] is False

    def test_missing_enabled_raises_error(self):
        """Test that missing enabled field raises ValueError."""
        with pytest.raises(ValueError, match="must include 'enabled' field"):
            DocumentationConfig(
                config={"documentation": {"markdown_checks": True}}  # Missing enabled
            )

    def test_frozen_dataclass(self):
        """Test that DocumentationConfig is immutable."""
        config = DocumentationConfig(config={"documentation": {"enabled": True}})
        with pytest.raises(AttributeError):
            config.config = {"modified": True}  # type: ignore
