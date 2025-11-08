"""Tests for configuration models."""

import pytest
from pydantic import ValidationError


def test_gitea_config_valid():
    """Test GiteaConfig with valid data."""
    from drep.models.config import GiteaConfig

    config = GiteaConfig(
        url="http://192.168.1.14:3000",
        token="test_token_123",
        repositories=["steve/*", "steve/drep"],
    )

    assert config.url == "http://192.168.1.14:3000"
    assert config.token.get_secret_value() == "test_token_123"
    assert config.repositories == ["steve/*", "steve/drep"]


def test_gitea_config_missing_required_fields():
    """Test GiteaConfig fails without required fields."""
    from drep.models.config import GiteaConfig

    with pytest.raises(ValidationError):
        GiteaConfig(url="http://192.168.1.14:3000")

    with pytest.raises(ValidationError):
        GiteaConfig(token="test_token")

    with pytest.raises(ValidationError):
        GiteaConfig(repositories=["steve/*"])


def test_documentation_config_defaults():
    """Test DocumentationConfig default values."""
    from drep.models.config import DocumentationConfig

    config = DocumentationConfig()

    assert config.enabled is True
    assert config.custom_dictionary == []


def test_documentation_config_custom_values():
    """Test DocumentationConfig with custom values."""
    from drep.models.config import DocumentationConfig

    config = DocumentationConfig(enabled=False, custom_dictionary=["asyncio", "gitea", "drep"])

    assert config.enabled is False
    assert config.custom_dictionary == ["asyncio", "gitea", "drep"]


def test_config_full_valid():
    """Test main Config with all sub-configs."""
    from drep.models.config import Config

    config_dict = {
        "gitea": {
            "url": "http://192.168.1.14:3000",
            "token": "test_token",
            "repositories": ["steve/*"],
        },
        "documentation": {"enabled": True, "custom_dictionary": ["asyncio"]},
        "database_url": "sqlite:///./test.db",
    }

    config = Config(**config_dict)

    assert config.gitea.url == "http://192.168.1.14:3000"
    assert config.gitea.token.get_secret_value() == "test_token"
    assert config.gitea.repositories == ["steve/*"]
    assert config.documentation.enabled is True
    assert config.documentation.custom_dictionary == ["asyncio"]
    assert config.database_url == "sqlite:///./test.db"


def test_config_default_database_url():
    """Test Config uses default database_url if not provided."""
    from drep.models.config import Config

    config_dict = {
        "gitea": {
            "url": "http://192.168.1.14:3000",
            "token": "test_token",
            "repositories": ["steve/*"],
        },
        "documentation": {"enabled": True},
    }

    config = Config(**config_dict)

    assert config.database_url == "sqlite:///./drep.db"


def test_config_serialization():
    """Test Config can be serialized to dict/JSON."""
    from drep.models.config import Config

    config_dict = {
        "gitea": {
            "url": "http://192.168.1.14:3000",
            "token": "test_token",
            "repositories": ["steve/*"],
        },
        "documentation": {"enabled": True, "custom_dictionary": ["asyncio"]},
    }

    config = Config(**config_dict)

    # Test model_dump() works
    dumped = config.model_dump()
    assert dumped["gitea"]["url"] == "http://192.168.1.14:3000"
    assert dumped["documentation"]["enabled"] is True


def test_gitea_config_field_descriptions():
    """Test that GiteaConfig has proper field descriptions."""
    from drep.models.config import GiteaConfig

    # Check that fields have descriptions
    schema = GiteaConfig.model_json_schema()

    assert "properties" in schema
    assert "url" in schema["properties"]
    assert "token" in schema["properties"]
    assert "repositories" in schema["properties"]


# ===== Config Platform Validator Tests =====


def test_config_requires_at_least_one_platform():
    """Test that Config raises ValueError when neither platform is configured."""
    from drep.models.config import Config

    with pytest.raises(ValueError, match="At least one platform must be configured"):
        Config()


def test_config_allows_gitea_only():
    """Test that Config accepts Gitea-only configuration."""
    from pydantic import SecretStr

    from drep.models.config import Config, GiteaConfig

    config = Config(
        gitea=GiteaConfig(
            url="http://localhost:3000", token=SecretStr("test_token"), repositories=["owner/*"]
        )
    )

    assert config.gitea is not None
    assert config.github is None


def test_config_allows_github_only():
    """Test that Config accepts GitHub-only configuration."""
    from pydantic import SecretStr

    from drep.models.config import Config, GitHubConfig

    config = Config(github=GitHubConfig(token=SecretStr("ghp_test"), repositories=["owner/*"]))

    assert config.github is not None
    assert config.gitea is None


def test_config_allows_both_platforms():
    """Test that Config accepts both Gitea and GitHub configuration."""
    from pydantic import SecretStr

    from drep.models.config import Config, GiteaConfig, GitHubConfig

    config = Config(
        gitea=GiteaConfig(
            url="http://localhost:3000", token=SecretStr("gitea_token"), repositories=["owner/*"]
        ),
        github=GitHubConfig(token=SecretStr("ghp_test"), repositories=["org/*"]),
    )

    assert config.gitea is not None
    assert config.github is not None


# ===== GitHubConfig SecretStr Tests =====


def test_github_config_token_is_secret_str():
    """Test that GitHubConfig.token is SecretStr and doesn't leak in repr."""
    from pydantic import SecretStr

    from drep.models.config import GitHubConfig

    config = GitHubConfig(token=SecretStr("ghp_secret_token_12345"), repositories=["owner/*"])

    # Token should be SecretStr
    assert isinstance(config.token, SecretStr)

    # Token should not appear in string representation
    repr_str = repr(config)
    assert "ghp_secret_token_12345" not in repr_str
    assert "**********" in repr_str or "SecretStr" in repr_str


def test_github_config_token_get_secret_value():
    """Test that GitHubConfig.token.get_secret_value() returns the actual token."""
    from pydantic import SecretStr

    from drep.models.config import GitHubConfig

    secret_token = "ghp_actual_token"
    config = GitHubConfig(token=SecretStr(secret_token), repositories=["owner/*"])

    assert config.token.get_secret_value() == secret_token


# ===== GitHubConfig HttpUrl Tests =====


def test_github_config_url_is_http_url():
    """Test that GitHubConfig.url validates as HttpUrl."""
    from pydantic import SecretStr, ValidationError

    from drep.models.config import GitHubConfig

    # Valid HTTP URL should work
    config = GitHubConfig(
        token=SecretStr("ghp_test"),
        repositories=["owner/*"],
        url="https://github.enterprise.com/api/v3",
    )

    assert str(config.url) == "https://github.enterprise.com/api/v3"

    # Invalid URL should raise ValidationError
    with pytest.raises(ValidationError):
        GitHubConfig(token=SecretStr("ghp_test"), repositories=["owner/*"], url="not-a-valid-url")


# ===== BedrockConfig Tests =====


def test_bedrock_config_defaults():
    """Test BedrockConfig with default values."""
    from drep.models.config import BedrockConfig

    config = BedrockConfig()

    assert config.region == "us-east-1"
    assert config.model == "anthropic.claude-sonnet-4-5-20250929-v1:0"


def test_bedrock_config_custom_values():
    """Test BedrockConfig with custom values."""
    from drep.models.config import BedrockConfig

    config = BedrockConfig(
        region="us-west-2",
        model="anthropic.claude-haiku-4-5-20251001-v1:0",
    )

    assert config.region == "us-west-2"
    assert config.model == "anthropic.claude-haiku-4-5-20251001-v1:0"


def test_bedrock_config_global_model_id():
    """Test BedrockConfig with global model ID format."""
    from drep.models.config import BedrockConfig

    config = BedrockConfig(
        region="eu-west-1",
        model="global.anthropic.claude-sonnet-4-5-20250929-v1:0",
    )

    assert config.region == "eu-west-1"
    assert config.model == "global.anthropic.claude-sonnet-4-5-20250929-v1:0"


# ===== LLMConfig Provider Field Tests =====


def test_llm_config_default_provider():
    """Test LLMConfig defaults to openai-compatible provider."""
    from drep.models.config import LLMConfig

    config = LLMConfig(
        enabled=True,
        endpoint="http://localhost:11434/v1",
        model="llama2",
    )

    assert config.provider == "openai-compatible"


def test_llm_config_bedrock_provider():
    """Test LLMConfig with bedrock provider."""
    from drep.models.config import BedrockConfig, LLMConfig

    config = LLMConfig(
        enabled=True,
        provider="bedrock",
        endpoint="http://localhost:11434/v1",  # Ignored for bedrock
        model="llama2",  # Ignored for bedrock
        bedrock=BedrockConfig(
            region="us-east-1",
            model="anthropic.claude-sonnet-4-5-20250929-v1:0",
        ),
    )

    assert config.provider == "bedrock"
    assert config.bedrock is not None
    assert config.bedrock.region == "us-east-1"
    assert config.bedrock.model == "anthropic.claude-sonnet-4-5-20250929-v1:0"


def test_llm_config_bedrock_without_bedrock_config():
    """Test LLMConfig with bedrock provider but no bedrock config raises ValidationError."""
    from pydantic import ValidationError

    from drep.models.config import LLMConfig

    # Should raise ValidationError at config time (fail fast)
    with pytest.raises(ValidationError, match="Bedrock provider requires 'bedrock' configuration"):
        LLMConfig(
            enabled=True,
            provider="bedrock",
            endpoint="http://localhost:11434/v1",
            model="llama2",
        )


def test_llm_config_backward_compatibility():
    """Test LLMConfig works without provider field (backward compatibility)."""
    from drep.models.config import LLMConfig

    config = LLMConfig(
        enabled=True,
        endpoint="http://localhost:11434/v1",
        model="llama2",
    )

    # Should default to openai-compatible
    assert config.provider == "openai-compatible"


# ===== Issue #1: Bedrock should not require endpoint/model =====


def test_llm_config_bedrock_without_endpoint_model():
    """Test Bedrock config works without endpoint/model (Issue #1 from PR review)."""
    from drep.models.config import BedrockConfig, LLMConfig

    # This should work - Bedrock doesn't need OpenAI endpoint/model
    config = LLMConfig(
        enabled=True,
        provider="bedrock",
        bedrock=BedrockConfig(
            region="us-east-1",
            model="anthropic.claude-sonnet-4-5-20250929-v1:0",
        ),
        temperature=0.2,
        max_tokens=4000,
    )

    assert config.provider == "bedrock"
    assert config.bedrock.region == "us-east-1"
    assert config.bedrock.model == "anthropic.claude-sonnet-4-5-20250929-v1:0"
    # endpoint and model should be None or have default values
    assert config.endpoint is None or config.endpoint
    assert config.model is None or config.model


def test_llm_config_openai_requires_endpoint_model():
    """Test OpenAI config fails without endpoint/model (Issue #1 validation)."""
    from pydantic import ValidationError

    from drep.models.config import LLMConfig

    # OpenAI-compatible provider MUST have endpoint and model
    with pytest.raises(ValidationError, match="endpoint"):
        LLMConfig(
            enabled=True,
            provider="openai-compatible",
            temperature=0.2,
        )

    with pytest.raises(ValidationError, match="model"):
        LLMConfig(
            enabled=True,
            provider="openai-compatible",
            endpoint="http://localhost:11434/v1",
            temperature=0.2,
        )
