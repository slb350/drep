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
    assert config.token == "test_token_123"
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
    assert config.gitea.token == "test_token"
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
