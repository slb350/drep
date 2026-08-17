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


def wizard_input(*answers: str) -> str:
    """Join wizard answers into the newline-delimited string CliRunner expects.

    The `drep init` wizard is a sequence of prompts, so tests drive it with a
    literal like "1\ngitea\n\n\nn\ny\nn\nn\nn\nn\n". Written inline that is
    opaque and the failure mode of an off-by-one is answering the wrong
    question silently. Prefer the named flows below, or spell the answers out
    one per argument so each is readable at the call site.
    """
    return "".join(f"{answer}\n" for answer in answers)


#: The most common flow: config in the current directory, Gitea with default
#: URL and repositories, no LLM, docs enabled, everything else declined.
GITEA_MINIMAL_INPUT = wizard_input(
    "1",  # config location: current directory
    "gitea",  # platform
    "",  # Gitea URL: default
    "",  # repositories: default
    "n",  # enable LLM
    "y",  # enable documentation analysis
    "n",  # markdown checks
    "n",  # custom dictionary
    "n",  # custom database URL
    "n",  # check env vars now
)
