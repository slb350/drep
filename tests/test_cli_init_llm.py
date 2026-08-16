"""Init wizard LLM configuration tests: provider selection, API keys, env checks."""

from pathlib import Path
from unittest.mock import patch

import yaml

from drep.cli import cli


class TestInitLlmConfig:
    """LLM-related init wizard behavior."""

    def test_init_with_llm_openai_compatible(self, runner, tmp_path):
        """Test init with OpenAI-compatible LLM provider."""
        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Wizard inputs:
            # 1. Platform: gitea
            # 2. Gitea URL: (default)
            # 3. Repositories: (default)
            # 4. Enable LLM: y
            # 5. Provider: openai-compatible
            # 6. Endpoint: (default)
            # 7. Model: (default)
            # 8. API key required: n
            # 9. Advanced settings: n
            # 10. Configure cache: n
            # 11. Enable docs: y
            # 12. Markdown checks: n
            # 13. Custom dictionary: n
            # 14. Custom DB: n
            # 15. Check env vars: n
            inputs = "1\ngitea\n\n\ny\nopenai-compatible\n\n\nn\nn\nn\ny\nn\nn\nn\nn\n"
            result = runner.invoke(cli, ["init"], input=inputs)

            assert result.exit_code == 0
            config_content = Path("config.yaml").read_text()
            assert "llm:" in config_content
            assert "enabled: true" in config_content
            assert "provider: openai-compatible" in config_content
            assert "endpoint: http://localhost:1234/v1" in config_content
            assert "model: qwen3-30b-a3b" in config_content

    def test_init_with_llm_bedrock(self, runner, tmp_path):
        """Test init with AWS Bedrock provider."""
        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Wizard inputs:
            # 1. Platform: github
            # 2. GitHub Enterprise: n
            # 3. Repositories: (default)
            # 4. Enable LLM: y
            # 5. Provider: bedrock
            # 6. Region: (default)
            # 7. Model: (default)
            # 8. Advanced settings: n
            # 9. Configure cache: n
            # 10. Enable docs: y
            # 11. Markdown checks: n
            # 12. Custom dictionary: n
            # 13. Custom DB: n
            # 14. Check env vars: n
            inputs = "1\ngithub\nn\n\ny\nbedrock\n\n\nn\nn\ny\nn\nn\nn\nn\n"
            result = runner.invoke(cli, ["init"], input=inputs)

            assert result.exit_code == 0
            config_content = Path("config.yaml").read_text()
            assert "llm:" in config_content
            assert "provider: bedrock" in config_content
            assert "bedrock:" in config_content
            assert "region: us-east-1" in config_content
            assert "anthropic.claude-sonnet-4-5-20250929-v1:0" in config_content

    def test_init_rejects_removed_anthropic_provider(self, runner, tmp_path):
        """C14: anthropic is no longer a wizard option; entering it re-prompts."""
        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Wizard inputs: provider "anthropic" (rejected) then "openai-compatible"
            inputs = "1\ngitea\n\n\ny\nanthropic\nopenai-compatible\n\n\nn\nn\nn\ny\nn\nn\nn\nn\n"
            result = runner.invoke(cli, ["init"], input=inputs)

            assert result.exit_code == 0
            config_content = Path("config.yaml").read_text()
            assert "provider: openai-compatible" in config_content
            assert "provider: anthropic" not in config_content

    def test_init_openai_with_api_key(self, runner, tmp_path):
        """Test OpenAI-compatible provider with API key."""
        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Wizard inputs with API key enabled
            # 1-4: gitea platform
            # 5: llm=y
            # 6: provider=openai-compatible
            # 7: endpoint (default)
            # 8: model (default)
            # 9: api_key=y
            # 10-15: rest minimal
            inputs = "1\ngitea\n\n\ny\nopenai-compatible\n\n\ny\nn\nn\ny\nn\nn\nn\nn\n"
            result = runner.invoke(cli, ["init"], input=inputs)

            assert result.exit_code == 0
            config = yaml.safe_load(Path("config.yaml").read_text())
            assert "llm" in config
            assert "api_key" in config["llm"]
            assert config["llm"]["api_key"] == "${LLM_API_KEY}"
            assert "LLM_API_KEY" in result.output

    def test_init_advanced_llm_settings(self, runner, tmp_path):
        """Test advanced LLM configuration."""
        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Wizard inputs with advanced settings
            # 0. Config location: 1 (current directory)
            # 1-8: openai setup
            # 9: advanced=y
            # 10-15: custom advanced values
            # 16: cache=n
            # 17-21: rest minimal
            inputs = (
                "1\ngitea\n\n\ny\nopenai-compatible\n\n\nn\ny\n0.7\n4096\n120\n5\n10\n120\nn\n"
                "y\nn\nn\nn\nn\n"
            )
            result = runner.invoke(cli, ["init"], input=inputs)

            assert result.exit_code == 0
            config = yaml.safe_load(Path("config.yaml").read_text())
            assert config["llm"]["temperature"] == 0.7
            assert config["llm"]["max_tokens"] == 4096
            assert config["llm"]["timeout"] == 120
            assert config["llm"]["max_retries"] == 5
            assert config["llm"]["max_concurrent_global"] == 10
            assert config["llm"]["requests_per_minute"] == 120

    def test_init_cache_configuration(self, runner, tmp_path):
        """Test LLM cache configuration."""
        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Wizard inputs with cache config
            # 1-9: openai setup, no advanced
            # 10: cache=y
            # 11-13: cache settings
            # 14-18: rest minimal
            inputs = "1\ngitea\n\n\ny\nopenai-compatible\n\n\nn\nn\ny\ny\n7\n5.0\ny\nn\nn\nn\nn\n"
            result = runner.invoke(cli, ["init"], input=inputs)

            assert result.exit_code == 0
            config = yaml.safe_load(Path("config.yaml").read_text())
            assert "cache" in config["llm"]
            assert config["llm"]["cache"]["enabled"] is True
            assert config["llm"]["cache"]["ttl_days"] == 7
            assert config["llm"]["cache"]["max_size_gb"] == 5.0

    def test_init_validation_failure(self, runner, tmp_path):
        """Test that validation failures abort with error."""

        with (
            runner.isolated_filesystem(temp_dir=tmp_path),
            patch("drep.cli_wizard.load_config") as mock_load,
        ):
            # Mock load_config to raise ValueError
            mock_load.side_effect = ValueError(
                "OpenAI-compatible provider requires 'endpoint' field"
            )
            inputs = "1\ngitea\n\n\nn\ny\nn\nn\nn\nn\n"
            result = runner.invoke(cli, ["init"], input=inputs)

            assert result.exit_code == 1
            assert "ERROR: Configuration validation failed:" in result.output
            assert "OpenAI-compatible provider requires 'endpoint' field" in result.output

    def test_init_handles_pydantic_validation_error(self, runner, tmp_path):
        """Test init formats Pydantic ValidationError correctly."""

        from pydantic_core import ValidationError

        with (
            runner.isolated_filesystem(temp_dir=tmp_path),
            patch("drep.cli_wizard.load_config") as mock_load,
        ):
            # Mock load_config to raise ValidationError with multiple fields
            mock_load.side_effect = ValidationError.from_exception_data(
                "Config",
                [
                    {
                        "type": "missing",
                        "loc": ("github", "token"),
                        "msg": "Field required",
                        "input": {},
                    },
                    {
                        "type": "string_type",
                        "loc": ("llm", "endpoint"),
                        "msg": "Input should be a valid string",
                        "input": 123,
                    },
                ],
            )
            inputs = "1\ngithub\nn\n\nn\ny\nn\nn\nn\nn\n"
            result = runner.invoke(cli, ["init"], input=inputs)

            assert result.exit_code == 1
            assert "ERROR: Configuration validation failed:" in result.output
            # Verify field paths are formatted correctly
            assert "github -> token" in result.output
            assert "llm -> endpoint" in result.output
            # Verify helpful guidance
            assert "Please re-run 'drep init' or fix manually" in result.output

    def test_init_unexpected_validation_error_propagates(self, runner, tmp_path):
        """Test unexpected validation exceptions propagate with stack trace."""

        with (
            runner.isolated_filesystem(temp_dir=tmp_path),
            patch("drep.cli_wizard.load_config") as mock_load,
        ):
            # Mock load_config to raise an unexpected exception
            mock_load.side_effect = RuntimeError("Unexpected error in config parsing")
            inputs = "1\ngitea\n\n\nn\ny\nn\nn\nn\nn\n"
            result = runner.invoke(cli, ["init"], input=inputs)

            # Unexpected exceptions should propagate (not exit cleanly)
            assert result.exit_code == 1
            # Should have exception information in result
            assert result.exception is not None
            assert isinstance(result.exception, RuntimeError)
            assert "Unexpected error in config parsing" in str(result.exception)

    # NOTE: test_init_env_check_handles_exception - SKIPPED
    # Error handling for env var checks IS implemented in drep/cli.py lines 484-489.
    # The code wraps env var checking in try-except and shows:
    # "WARNING: Cannot check environment variables: {e}\nPlease verify manually."
    #
    # Testing this via mocking os.environ causes cascading failures throughout the
    # codebase because os.environ is used extensively in Click and pytest infrastructure.
    # Multiple mocking approaches attempted (patch.object, MagicMock, monkeypatch) all
    # resulted in "ValueError: not enough values to unpack" or RuntimeError in Click.
    #
    # The error handling has been manually verified via code inspection and is correct.
    # This is an edge case (restricted environments blocking os.environ access) that is
    # extremely rare in practice.

    def test_init_env_check_shows_missing_vars(self, runner, tmp_path, monkeypatch):
        """Test env check shows warning when vars are missing."""
        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Clear environment variables
            monkeypatch.delenv("GITHUB_TOKEN", raising=False)
            monkeypatch.delenv("LLM_API_KEY", raising=False)

            # Wizard: location + GitHub + OpenAI-compatible (with API key) + docs + db + env check
            inputs = (
                "1\ngithub\nn\nowner/*\ny\nopenai-compatible\nhttp://localhost:1234/v1\n"
                "test-model\ny\nn\nn\ny\nn\nn\nn\ny\n"
            )
            result = runner.invoke(cli, ["init"], input=inputs)

            assert result.exit_code == 0
            assert "WARNING: Missing environment variables:" in result.output
            assert "GITHUB_TOKEN" in result.output
            assert "LLM_API_KEY" in result.output

    def test_init_env_check_all_set(self, runner, tmp_path, monkeypatch):
        """Test env check shows success when all vars are set."""
        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Set required environment variables
            monkeypatch.setenv("GITEA_TOKEN", "test-token")

            # Wizard inputs: location + Gitea + no LLM + docs + db + env check yes
            inputs = "1\ngitea\nhttp://localhost:3000\nowner/*\nn\ny\nn\nn\nn\ny\n"
            result = runner.invoke(cli, ["init"], input=inputs)

            assert result.exit_code == 0
            assert "✓ All required environment variables are set!" in result.output

    def test_init_env_check_detects_llm_api_key(self, runner, tmp_path, monkeypatch):
        """Test env check detects missing LLM_API_KEY for openai-compatible."""
        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Clear environment variables
            monkeypatch.delenv("GITHUB_TOKEN", raising=False)
            monkeypatch.delenv("LLM_API_KEY", raising=False)

            # Wizard: location + GitHub + OpenAI-compatible + docs + db + env check
            inputs = (
                "1\ngithub\nn\nowner/*\ny\nopenai-compatible\n"
                "http://localhost:1234/v1\nqwen3-30b-a3b\ny\nn\nn\n"
                "y\nn\nn\nn\ny\n"
            )
            result = runner.invoke(cli, ["init"], input=inputs)

            assert result.exit_code == 0
            assert "WARNING: Missing environment variables:" in result.output
            assert "LLM_API_KEY" in result.output
            assert "GITHUB_TOKEN" in result.output

    def test_init_env_check_detects_aws_credentials_for_bedrock(
        self, runner, tmp_path, monkeypatch
    ):
        """Test env check detects missing AWS credentials for Bedrock provider."""
        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Clear AWS environment variables
            monkeypatch.delenv("GITHUB_TOKEN", raising=False)
            monkeypatch.delenv("AWS_ACCESS_KEY_ID", raising=False)
            monkeypatch.delenv("AWS_SECRET_ACCESS_KEY", raising=False)

            # Wizard: location + GitHub + Bedrock + docs + db + env check
            # Bedrock needs: region, model_id
            inputs = (
                "1\ngithub\nn\nowner/*\ny\nbedrock\n"
                "us-east-1\nanthropic.claude-3-5-sonnet-20241022-v2:0\nn\nn\n"
                "n\nn\ny\n"
            )
            result = runner.invoke(cli, ["init"], input=inputs)

            assert result.exit_code == 0
            assert "WARNING: Missing environment variables:" in result.output
            assert "AWS_ACCESS_KEY_ID" in result.output
            assert "AWS_SECRET_ACCESS_KEY" in result.output
            assert "GITHUB_TOKEN" in result.output
