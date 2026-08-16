"""End-to-end wizard-to-config workflows and platform resolver tests."""

from pathlib import Path

import click
import pytest

from drep.cli import cli


class TestEndToEndIntegration:
    """INTEGRATION: Test complete workflow from wizard → load → validate → use.

    These tests verify Issue #3 from PR review - the entire pipeline works:
    wizard creates config → config loads correctly → config validates → scan can use it.
    """

    def test_github_end_to_end_workflow(self, runner, tmp_path, monkeypatch):
        """Test GitHub config created by wizard loads and validates correctly.

        Integration test: Verifies entire pipeline from wizard → load → validate.
        """
        monkeypatch.setenv("GITHUB_TOKEN", "test_token_value")

        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Step 1: Create config via wizard
            inputs = "1\ngithub\nn\nowner/*\nn\ny\nn\nn\nn\nn\n"
            result = runner.invoke(cli, ["init"], input=inputs)
            assert result.exit_code == 0

            # Step 2: Verify config file exists
            config_path = Path("config.yaml")
            assert config_path.exists()

            # Step 3: Load config using load_config()
            from drep.config import load_config

            config = load_config(str(config_path))

            # Step 4: Verify structure
            assert config.github is not None
            assert config.github.token.get_secret_value() == "test_token_value"
            assert config.github.repositories == ["owner/*"]
            assert config.github.url == "https://api.github.com"  # Default GitHub.com API

            # Step 5: Verify config is usable (adapter can be created)
            from drep.adapters.github import GitHubAdapter

            adapter = GitHubAdapter(
                token=config.github.token.get_secret_value(),
                url=str(config.github.url) if config.github.url else None,
            )
            assert adapter is not None

    def test_gitea_with_bedrock_end_to_end(self, runner, tmp_path, monkeypatch):
        """Test Gitea + Bedrock config workflow (complex nested config)."""
        monkeypatch.setenv("GITEA_TOKEN", "test_gitea_token")

        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Create Gitea + Bedrock config
            inputs = (
                "1\ngitea\nhttp://localhost:3000\nowner/*\n"  # Gitea
                "y\nbedrock\nus-east-1\nanthropic.claude-sonnet-4-5-20250929-v1:0\n"  # Bedrock
                "n\nn\nn\nn\nn\n"  # Skip advanced/cache/doc
            )
            result = runner.invoke(cli, ["init"], input=inputs)
            assert result.exit_code == 0

            # Load and verify
            from drep.config import load_config

            config = load_config("config.yaml")

            # Verify Gitea config
            assert config.gitea is not None
            assert config.gitea.token.get_secret_value() == "test_gitea_token"
            assert config.gitea.url == "http://localhost:3000"
            assert config.gitea.repositories == ["owner/*"]

            # Verify Bedrock LLM config
            assert config.llm is not None
            assert config.llm.provider == "bedrock"
            assert config.llm.bedrock.region == "us-east-1"
            assert "anthropic.claude" in config.llm.bedrock.model

            # Verify adapter can be created
            from drep.adapters.gitea import GiteaAdapter

            adapter = GiteaAdapter(
                url=config.gitea.url,
                token=config.gitea.token.get_secret_value(),
            )
            assert adapter is not None

    def test_gitlab_with_openai_compatible_end_to_end(self, runner, tmp_path, monkeypatch):
        """Test GitLab + OpenAI-compatible LLM config workflow."""
        monkeypatch.setenv("GITLAB_TOKEN", "test_gitlab_token")
        monkeypatch.setenv("LLM_API_KEY", "test_llm_key")

        with runner.isolated_filesystem(temp_dir=tmp_path):
            inputs = (
                "1\ngitlab\nn\ngroup/*\n"  # GitLab
                "y\nopenai-compatible\nhttp://localhost:1234/v1\ntest-model\ny\n"  # LLM + api key
                "n\nn\nn\nn\nn\n"  # Skip advanced
            )
            result = runner.invoke(cli, ["init"], input=inputs)
            assert result.exit_code == 0

            from drep.config import load_config

            config = load_config("config.yaml")

            # Verify GitLab config
            assert config.gitlab is not None
            assert config.gitlab.token.get_secret_value() == "test_gitlab_token"
            assert config.gitlab.repositories == ["group/*"]

            # Verify LLM config
            assert config.llm is not None
            assert config.llm.provider == "openai-compatible"
            assert config.llm.api_key == "test_llm_key"
            assert config.llm.model == "test-model"

            # Verify adapter can be created
            from drep.adapters.gitlab import GitLabAdapter

            adapter = GitLabAdapter(
                token=config.gitlab.token.get_secret_value(),
                url=str(config.gitlab.url) if config.gitlab.url else None,
            )
            assert adapter is not None

    def test_config_with_custom_database_end_to_end(self, runner, tmp_path, monkeypatch):
        """Test config with custom database URL."""
        monkeypatch.setenv("GITHUB_TOKEN", "test_token")

        with runner.isolated_filesystem(temp_dir=tmp_path):
            inputs = (
                "1\ngithub\nn\nowner/*\n"  # GitHub
                "n\n"  # No LLM
                "y\nn\nn\n"  # Doc: enabled, no markdown, no dict
                "y\nsqlite:///custom.db\n"  # Custom database
                "n\n"  # No env check
            )
            result = runner.invoke(cli, ["init"], input=inputs)
            assert result.exit_code == 0

            from drep.config import load_config

            config = load_config("config.yaml")

            # Verify custom database
            assert config.database_url == "sqlite:///custom.db"

    def test_config_validation_catches_malformed_yaml(self, runner, tmp_path):
        """Test that malformed YAML is caught gracefully."""
        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Create intentionally broken YAML
            config_path = Path("config.yaml")
            config_path.write_text("invalid: yaml: structure: [unclosed")

            # Try to validate
            result = runner.invoke(cli, ["validate", str(config_path)])

            # Should fail with validation error
            assert result.exit_code != 0
            # Error message should be helpful (not a stack trace)
            assert "error" in result.output.lower() or "invalid" in result.output.lower()


class TestResolvePlatform:
    """C17: platform selection is single-sourced in _resolve_platform.

    Both the scan and review workflows duplicated the gitea>github>gitlab
    chain; precedence bugs could diverge between them.
    """

    def _config(self, tmp_path, yaml_text):
        from drep.config import load_config

        cfg = tmp_path / "config.yaml"
        cfg.write_text(yaml_text)
        return load_config(str(cfg))

    def test_prefers_gitea_when_all_configured(self, tmp_path):
        from drep.cli_workflows import _resolve_platform

        config = self._config(
            tmp_path,
            """
gitea:
  url: http://localhost:3000
  token: t-gitea
  repositories: ["o/*"]
github:
  token: t-github
  repositories: ["o/*"]
gitlab:
  token: t-gitlab
  repositories: ["o/*"]
""",
        )
        name, _adapter, git_url, token = _resolve_platform(config, "o", "r")
        assert name == "gitea"
        assert "localhost:3000" in git_url
        assert token == "t-gitea"

    def test_github_fallback_and_url(self, tmp_path):
        from drep.cli_workflows import _resolve_platform

        config = self._config(
            tmp_path,
            """
github:
  token: t-github
  repositories: ["o/*"]
""",
        )
        name, _adapter, git_url, token = _resolve_platform(config, "o", "r")
        assert name == "github"
        assert git_url == "https://github.com/o/r.git"
        assert token == "t-github"

    def test_gitlab_selfhosted_url(self, tmp_path):
        from drep.cli_workflows import _resolve_platform

        config = self._config(
            tmp_path,
            """
gitlab:
  token: t-gitlab
  url: https://gitlab.example.com
  repositories: ["o/*"]
""",
        )
        name, _adapter, git_url, _token = _resolve_platform(config, "o", "r")
        assert name == "gitlab"
        assert git_url == "https://gitlab.example.com/o/r.git"

    def test_no_platform_aborts(self, tmp_path):
        from drep.cli_workflows import _resolve_platform
        from drep.models.config import Config

        config = Config(gitea=None, github=None, gitlab=None, require_platform_config=False)
        with pytest.raises(click.Abort):
            _resolve_platform(config, "o", "r")
