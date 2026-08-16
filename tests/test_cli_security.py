"""Token leakage prevention tests: secrets never appear in output, logs, or config files."""

from pathlib import Path

from drep.cli import cli


class TestTokenLeakagePrevention:
    """SECURITY: Test that API tokens never leak into logs, stdout, or error messages.

    CRITICAL: These tests verify Issue #2 from PR review - tokens must never
    appear in application output where they could be exposed in CI logs,
    monitoring systems, or support tickets.
    """

    def test_init_never_logs_actual_token_values(self, runner, tmp_path, monkeypatch, caplog):
        """Test wizard never logs environment variable values.

        Security test: Ensures tokens don't leak into application logs
        where they could be exposed in CI logs, monitoring systems, etc.
        """
        # Set up environment with real token value
        monkeypatch.setenv("GITHUB_TOKEN", "secret_ghp_token_12345")

        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Run init wizard with GitHub platform
            inputs = "1\ngithub\nn\nowner/*\nn\ny\nn\nn\nn\nn\n"
            result = runner.invoke(cli, ["init"], input=inputs)

            # Verify success
            assert result.exit_code == 0

            # CRITICAL: Actual token value must NEVER appear in output
            assert "secret_ghp_token_12345" not in result.output

            # CRITICAL: Actual token value must NEVER appear in logs
            log_output = caplog.text
            assert "secret_ghp_token_12345" not in log_output

            # Verify config file has placeholder, not actual value
            config_content = Path("config.yaml").read_text()
            assert "${GITHUB_TOKEN}" in config_content
            assert "secret_ghp_token_12345" not in config_content

    def test_init_gitea_never_logs_token(self, runner, tmp_path, monkeypatch, caplog):
        """Test Gitea wizard never logs token values."""
        monkeypatch.setenv("GITEA_TOKEN", "actual_secret_gitea_token_xyz")

        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Run wizard with Gitea platform
            inputs = "1\ngitea\nhttp://localhost:3000\nowner/*\nn\ny\nn\nn\nn\nn\n"
            result = runner.invoke(cli, ["init"], input=inputs)

            assert result.exit_code == 0

            # CRITICAL: Actual token must NEVER appear in output
            assert "actual_secret_gitea_token_xyz" not in result.output

            # CRITICAL: Actual token must NEVER appear in logs
            assert "actual_secret_gitea_token_xyz" not in caplog.text

            # Check config file has placeholder
            config_content = Path("config.yaml").read_text()
            assert "${GITEA_TOKEN}" in config_content
            assert "actual_secret_gitea_token_xyz" not in config_content

    def test_init_llm_api_key_never_logged(self, runner, tmp_path, monkeypatch, caplog):
        """Test LLM API key never appears in logs or output."""
        monkeypatch.setenv("GITHUB_TOKEN", "test_github")
        monkeypatch.setenv("LLM_API_KEY", "sk-llm-secret-key-12345")

        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Run wizard with GitHub + OpenAI-compatible LLM requiring an API key
            inputs = (
                "1\ngithub\nn\nowner/*\n"  # GitHub platform
                "y\nopenai-compatible\nhttp://localhost:1234/v1\ntest-model\ny\n"  # LLM + api key
                "n\nn\nn\nn\nn\n"  # Skip advanced settings
            )
            result = runner.invoke(cli, ["init"], input=inputs)

            assert result.exit_code == 0

            # CRITICAL: API key must NEVER appear in output
            assert "sk-llm-secret-key-12345" not in result.output

            # CRITICAL: API key must NEVER appear in logs
            assert "sk-llm-secret-key-12345" not in caplog.text

            # Check config file has placeholder
            config_content = Path("config.yaml").read_text()
            assert "${LLM_API_KEY}" in config_content
            assert "sk-llm-secret-key-12345" not in config_content

    def test_init_env_check_masks_token_values_in_output(self, runner, tmp_path, monkeypatch):
        """Test environment variable verification doesn't leak values."""
        monkeypatch.setenv("GITLAB_TOKEN", "actual_secret_value_123")

        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Run wizard with GitLab
            inputs = "1\ngitlab\nn\ngroup/*\nn\ny\nn\nn\nn\nn\n"
            result = runner.invoke(cli, ["init"], input=inputs)

            assert result.exit_code == 0

            # CRITICAL: Actual token value must NEVER appear anywhere
            assert "actual_secret_value_123" not in result.output

            # Env var name is OK to appear (not the value)
            assert "GITLAB_TOKEN" in result.output

    def test_init_multiple_tokens_all_masked(self, runner, tmp_path, monkeypatch, caplog):
        """Test wizard with multiple tokens never leaks any of them."""
        # Set multiple environment variables
        monkeypatch.setenv("GITHUB_TOKEN", "secret_github_xyz")
        monkeypatch.setenv("LLM_API_KEY", "secret_llm_abc")

        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Run wizard with GitHub + OpenAI-compatible LLM requiring an API key
            inputs = (
                "1\ngithub\nn\nowner/*\n"
                "y\nopenai-compatible\nhttp://localhost:1234/v1\ntest-model\ny\n"
                "n\nn\nn\nn\nn\n"
            )
            result = runner.invoke(cli, ["init"], input=inputs)

            assert result.exit_code == 0

            # Neither token should appear in output
            assert "secret_github_xyz" not in result.output
            assert "secret_llm_abc" not in result.output

            # Neither token should appear in logs
            assert "secret_github_xyz" not in caplog.text
            assert "secret_llm_abc" not in caplog.text

            # Config should only have placeholders
            config_content = Path("config.yaml").read_text()
            assert "secret_github_xyz" not in config_content
            assert "secret_llm_abc" not in config_content
            assert "${GITHUB_TOKEN}" in config_content
            assert "${LLM_API_KEY}" in config_content
