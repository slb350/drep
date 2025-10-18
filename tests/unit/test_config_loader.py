"""Tests for configuration loading."""

import os

import pytest
from pydantic import ValidationError


@pytest.fixture
def config_file(tmp_path):
    """Create a sample config file for testing."""
    config_path = tmp_path / "config.yaml"
    config_content = """gitea:
  url: http://192.168.1.14:3000
  token: test_token_123
  repositories:
    - steve/*
    - steve/drep

documentation:
  enabled: true
  custom_dictionary:
    - asyncio
    - gitea

database_url: sqlite:///./drep.db
"""
    config_path.write_text(config_content)
    return config_path


@pytest.fixture
def config_with_env_vars(tmp_path):
    """Create a config file with environment variables."""
    config_path = tmp_path / "config.yaml"
    config_content = """gitea:
  url: ${GITEA_URL}
  token: ${GITEA_TOKEN}
  repositories:
    - steve/*

documentation:
  enabled: true
"""
    config_path.write_text(config_content)
    return config_path


def test_load_config_basic(config_file):
    """Test loading a basic configuration file."""
    from drep.config import load_config

    config = load_config(str(config_file))

    assert config.gitea.url == "http://192.168.1.14:3000"
    assert config.gitea.token == "test_token_123"
    assert config.gitea.repositories == ["steve/*", "steve/drep"]
    assert config.documentation.enabled is True
    assert config.documentation.custom_dictionary == ["asyncio", "gitea"]
    assert config.database_url == "sqlite:///./drep.db"


def test_load_config_with_env_vars(config_with_env_vars):
    """Test loading config with environment variable substitution."""
    from drep.config import load_config

    # Set environment variables
    os.environ["GITEA_URL"] = "http://test.com:3000"
    os.environ["GITEA_TOKEN"] = "secret_token_456"

    try:
        config = load_config(str(config_with_env_vars))

        assert config.gitea.url == "http://test.com:3000"
        assert config.gitea.token == "secret_token_456"
    finally:
        # Clean up
        del os.environ["GITEA_URL"]
        del os.environ["GITEA_TOKEN"]


def test_load_config_env_var_not_set(config_with_env_vars):
    """Test that missing env vars are left as-is in the config."""
    from drep.config import load_config

    # Make sure env vars are NOT set
    os.environ.pop("GITEA_URL", None)
    os.environ.pop("GITEA_TOKEN", None)

    config = load_config(str(config_with_env_vars))

    # Should remain as ${VAR_NAME} if not set
    assert config.gitea.url == "${GITEA_URL}"
    assert config.gitea.token == "${GITEA_TOKEN}"


def test_load_config_file_not_found():
    """Test that FileNotFoundError is raised for missing config."""
    from drep.config import load_config

    with pytest.raises(FileNotFoundError) as exc_info:
        load_config("/nonexistent/config.yaml")

    assert "not found" in str(exc_info.value)


def test_load_config_invalid_yaml(tmp_path):
    """Test that invalid YAML raises an error."""
    import yaml

    from drep.config import load_config

    config_path = tmp_path / "bad.yaml"
    config_path.write_text("gitea:\n  url: http://example.com\n    token: invalid indentation")

    with pytest.raises(yaml.YAMLError):
        load_config(str(config_path))


def test_load_config_validation_error(tmp_path):
    """Test that invalid config structure raises ValidationError."""
    from drep.config import load_config

    config_path = tmp_path / "invalid.yaml"
    # Missing required fields
    config_content = """gitea:
  url: http://192.168.1.14:3000
  # Missing token and repositories
"""
    config_path.write_text(config_content)

    with pytest.raises(ValidationError):
        load_config(str(config_path))


def test_load_config_default_database_url(tmp_path):
    """Test that database_url defaults if not specified."""
    from drep.config import load_config

    config_path = tmp_path / "config.yaml"
    config_content = """gitea:
  url: http://192.168.1.14:3000
  token: test_token
  repositories:
    - steve/*

documentation:
  enabled: true
"""
    config_path.write_text(config_content)

    config = load_config(str(config_path))

    assert config.database_url == "sqlite:///./drep.db"


def test_load_config_documentation_defaults(tmp_path):
    """Test that documentation config has defaults."""
    from drep.config import load_config

    config_path = tmp_path / "config.yaml"
    config_content = """gitea:
  url: http://192.168.1.14:3000
  token: test_token
  repositories:
    - steve/*
"""
    config_path.write_text(config_content)

    # Should raise ValidationError because documentation is required
    with pytest.raises(ValidationError):
        load_config(str(config_path))


def test_load_config_complex_env_vars(tmp_path):
    """Test environment variable substitution in nested structures."""
    from drep.config import load_config

    config_path = tmp_path / "config.yaml"
    config_content = """gitea:
  url: ${GITEA_URL}
  token: ${GITEA_TOKEN}
  repositories:
    - ${REPO_PATTERN}

documentation:
  enabled: true
  custom_dictionary:
    - ${CUSTOM_WORD}

database_url: ${DATABASE_URL}
"""
    config_path.write_text(config_content)

    # Set environment variables
    os.environ["GITEA_URL"] = "http://test.com"
    os.environ["GITEA_TOKEN"] = "token123"
    os.environ["REPO_PATTERN"] = "steve/*"
    os.environ["CUSTOM_WORD"] = "pytest"
    os.environ["DATABASE_URL"] = "sqlite:///test.db"

    try:
        config = load_config(str(config_path))

        assert config.gitea.url == "http://test.com"
        assert config.gitea.token == "token123"
        assert config.gitea.repositories == ["steve/*"]
        assert config.documentation.custom_dictionary == ["pytest"]
        assert config.database_url == "sqlite:///test.db"
    finally:
        # Clean up
        for key in ["GITEA_URL", "GITEA_TOKEN", "REPO_PATTERN", "CUSTOM_WORD", "DATABASE_URL"]:
            del os.environ[key]
