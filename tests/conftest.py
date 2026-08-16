"""Shared fixtures for drep tests."""

import pytest
import yaml
from click.testing import CliRunner


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
