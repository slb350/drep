"""Shared fixtures for drep tests."""

from unittest.mock import AsyncMock, MagicMock

import pytest
import yaml
from click.testing import CliRunner


@pytest.fixture
def mock_llm_client():
    """Mock LLM client whose analyze_code_json is an AsyncMock."""
    client = MagicMock()
    client.analyze_code_json = AsyncMock()
    return client


@pytest.fixture
def runner():
    """Create a Click CLI test runner."""
    return CliRunner()


@pytest.fixture
def temp_config_file(tmp_path):
    """Create temporary config file."""
    config_path = tmp_path / "config.yaml"
    config_data = {
        "gitea": {
            "url": "http://192.168.1.14:3000",
            "token": "test-token",
            "repositories": ["steve/*"],
        },
        "documentation": {
            "enabled": True,
            "custom_dictionary": ["asyncio", "fastapi", "gitea"],
        },
        "database_url": "sqlite:///./drep.db",
    }
    config_path.write_text(yaml.dump(config_data))
    return config_path
