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


# ===== Tests for Strongly-Typed Models (Fix #25) =====


class TestGitHubPlatformData:
    """Tests for GitHubPlatformData model."""

    def test_github_platform_data_immutable(self):
        """Test GitHubPlatformData is frozen (immutable)."""
        from drep.models.wizard import GitHubPlatformData

        data = GitHubPlatformData(
            token="${GITHUB_TOKEN}",
            repositories=("owner/repo1", "owner/repo2"),
            url="https://github.example.com/api/v3",
        )

        # Should be frozen (Python 3.13 raises FrozenInstanceError,
        # earlier versions raise AttributeError)
        with pytest.raises(
            (AttributeError, Exception), match="can't set attribute|frozen|cannot assign"
        ):
            data.token = "new-token"

    def test_github_platform_data_uses_tuples(self):
        """Test repositories stored as tuple (immutable)."""
        from drep.models.wizard import GitHubPlatformData

        data = GitHubPlatformData(
            token="${GITHUB_TOKEN}",
            repositories=("owner/repo",),
        )

        assert isinstance(data.repositories, tuple)
        # Tuples don't have append
        assert not hasattr(data.repositories, "append")

    def test_github_platform_data_to_dict(self):
        """Test to_dict() YAML serialization."""
        from drep.models.wizard import GitHubPlatformData

        data = GitHubPlatformData(
            token="${GITHUB_TOKEN}",
            repositories=("owner/repo1", "owner/repo2"),
            url="https://github.example.com/api/v3",
        )

        result = data.to_dict()

        # Should convert tuple to list for YAML
        assert result == {
            "token": "${GITHUB_TOKEN}",
            "repositories": ["owner/repo1", "owner/repo2"],
            "url": "https://github.example.com/api/v3",
        }

    def test_github_platform_data_optional_url(self):
        """Test GitHubPlatformData without url (github.com, not enterprise)."""
        from drep.models.wizard import GitHubPlatformData

        data = GitHubPlatformData(
            token="${GITHUB_TOKEN}",
            repositories=("owner/*",),
        )

        result = data.to_dict()

        # Should not include url key when None
        assert "url" not in result
        assert result == {"token": "${GITHUB_TOKEN}", "repositories": ["owner/*"]}


class TestPlatformConfigStronglyTyped:
    """Tests for PlatformConfig wrapper with strongly-typed data."""

    def test_platform_config_with_github_data(self):
        """Test PlatformConfig with GitHubPlatformData."""
        from drep.models.wizard import GitHubPlatformData, PlatformConfig

        github_data = GitHubPlatformData(token="${GITHUB_TOKEN}", repositories=("owner/*",))

        config = PlatformConfig(data=github_data, env_var="GITHUB_TOKEN", platform_name="GitHub")

        assert config.platform_name == "GitHub"
        assert config.env_var == "GITHUB_TOKEN"
        assert isinstance(config.data, GitHubPlatformData)

    def test_platform_config_to_dict(self):
        """Test PlatformConfig.to_dict() creates correct structure."""
        from drep.models.wizard import GiteaPlatformData, PlatformConfig

        gitea_data = GiteaPlatformData(
            url="http://192.168.1.14:3000",
            token="${GITEA_TOKEN}",
            repositories=("steve/*",),
        )

        config = PlatformConfig(data=gitea_data, env_var="GITEA_TOKEN", platform_name="Gitea")

        result = config.to_dict()

        # Should nest under "gitea" key
        assert result == {
            "gitea": {
                "url": "http://192.168.1.14:3000",
                "token": "${GITEA_TOKEN}",
                "repositories": ["steve/*"],
            }
        }


class TestBedrockRegionModel:
    """Tests for BedrockRegionModel."""

    def test_bedrock_region_model_immutable(self):
        """Test BedrockRegionModel is frozen."""
        from drep.models.wizard import BedrockRegionModel

        model = BedrockRegionModel(region="us-east-1", model="anthropic.claude-v2")

        with pytest.raises(
            (AttributeError, Exception), match="can't set attribute|frozen|cannot assign"
        ):
            model.region = "us-west-2"

    def test_bedrock_region_model_to_dict(self):
        """Test BedrockRegionModel.to_dict() serialization."""
        from drep.models.wizard import BedrockRegionModel

        model = BedrockRegionModel(
            region="us-east-1", model="anthropic.claude-sonnet-4-5-20250929-v1:0"
        )

        result = model.to_dict()

        assert result == {
            "region": "us-east-1",
            "model": "anthropic.claude-sonnet-4-5-20250929-v1:0",
        }


class TestLLMConfigModels:
    """Tests for LLM configuration models."""

    def test_openai_llm_data_immutable(self):
        """Test OpenAILLMData is frozen."""
        from drep.models.wizard import OpenAILLMData

        data = OpenAILLMData(
            enabled=True,
            provider="openai-compatible",
            endpoint="http://localhost:1234/v1",
            model="qwen3-30b-a3b",
        )

        with pytest.raises(
            (AttributeError, Exception), match="can't set attribute|frozen|cannot assign"
        ):
            data.enabled = False

    def test_llm_config_provider_property(self):
        """Test LLMConfig.provider is derived from data."""
        from drep.models.wizard import BedrockLLMData, BedrockRegionModel, LLMConfig

        bedrock_data = BedrockLLMData(
            enabled=True,
            provider="bedrock",
            bedrock=BedrockRegionModel(
                region="us-east-1",
                model="anthropic.claude-sonnet-4-5-20250929-v1:0",
            ),
        )

        config = LLMConfig(data=bedrock_data)

        # Provider should be derived from data
        assert config.provider == "bedrock"

    def test_llm_config_to_dict(self):
        """Test LLMConfig.to_dict() creates correct structure."""
        from drep.models.wizard import AnthropicLLMData, LLMConfig

        anthropic_data = AnthropicLLMData(
            enabled=True,
            provider="anthropic",
            api_key="${ANTHROPIC_API_KEY}",
            model="claude-sonnet-4-5-20250929",
            temperature=0.7,
        )

        config = LLMConfig(data=anthropic_data)
        result = config.to_dict()

        # Should nest under "llm" key
        assert result == {
            "llm": {
                "enabled": True,
                "provider": "anthropic",
                "api_key": "${ANTHROPIC_API_KEY}",
                "model": "claude-sonnet-4-5-20250929",
                "temperature": 0.7,
            }
        }

    def test_openai_llm_data_omits_none_fields(self):
        """Test OpenAILLMData.to_dict() omits optional fields when None."""
        from drep.models.wizard import OpenAILLMData

        data = OpenAILLMData(
            enabled=True,
            provider="openai-compatible",
            endpoint="http://localhost:1234/v1",
            model="qwen3-30b-a3b",
            # All optional fields left as None
        )

        result = data.to_dict()

        # Should only include required fields
        assert result == {
            "enabled": True,
            "provider": "openai-compatible",
            "endpoint": "http://localhost:1234/v1",
            "model": "qwen3-30b-a3b",
        }
        # Optional fields should not be present
        assert "api_key" not in result
        assert "temperature" not in result
        assert "max_tokens" not in result


class TestDocumentationConfigStronglyTyped:
    """Tests for DocumentationConfigData model."""

    def test_documentation_config_data_immutable(self):
        """Test DocumentationConfigData is frozen."""
        from drep.models.wizard import DocumentationConfigData

        data = DocumentationConfigData(
            enabled=True,
            markdown_checks=True,
            custom_dictionary=("word1", "word2"),
        )

        with pytest.raises(
            (AttributeError, Exception), match="can't set attribute|frozen|cannot assign"
        ):
            data.enabled = False

    def test_documentation_config_data_uses_tuples(self):
        """Test custom_dictionary stored as tuple (immutable)."""
        from drep.models.wizard import DocumentationConfigData

        data = DocumentationConfigData(enabled=True, custom_dictionary=("word1", "word2"))

        assert isinstance(data.custom_dictionary, tuple)
        assert not hasattr(data.custom_dictionary, "append")

    def test_documentation_config_to_dict(self):
        """Test DocumentationConfig.to_dict() creates correct structure."""
        from drep.models.wizard import DocumentationConfig, DocumentationConfigData

        doc_data = DocumentationConfigData(
            enabled=True,
            markdown_checks=True,
            custom_dictionary=("asyncio", "drep"),
        )

        config = DocumentationConfig(data=doc_data)
        result = config.to_dict()

        # Should nest under "documentation" key and convert tuple to list
        assert result == {
            "documentation": {
                "enabled": True,
                "markdown_checks": True,
                "custom_dictionary": ["asyncio", "drep"],
            }
        }
