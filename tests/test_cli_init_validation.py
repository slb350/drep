"""Init wizard validation tests: config discovery, URLs, advanced settings."""

from pathlib import Path
from unittest.mock import AsyncMock, patch

from drep.cli import cli


class TestConfigDiscoveryConsistency:
    """Tests verifying init and scan use consistent config discovery."""

    def test_init_and_scan_config_discovery_consistency(self, runner, tmp_path):
        """Test that init creates config where scan discovers it.

        This verifies that drep init and drep scan use consistent config
        discovery logic - configs created by init should be found by scan.
        """
        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Run drep init with location choice "1" (current directory)
            # This should create ./config.yaml
            inputs = "1\ngitea\n\n\nn\ny\nn\nn\nn\nn\n"
            result = runner.invoke(cli, ["init"], input=inputs)

            assert result.exit_code == 0
            assert Path("config.yaml").exists()

            # Verify find_config_file() would discover this config
            from drep.config import find_config_file

            discovered_path = find_config_file(None)  # No explicit path
            assert discovered_path == Path("config.yaml")
            assert discovered_path.exists()

            # Verify scan command would find this config
            # (Mock the actual scan to avoid needing a real repository)
            with patch("drep.cli_workflows._run_scan", new_callable=AsyncMock) as mock_scan:
                result = runner.invoke(cli, ["scan", "owner/repo"])

                # Should succeed because config is discovered
                assert result.exit_code == 0
                # Verify scan was called with the discovered config path
                mock_scan.assert_called_once_with("owner", "repo", "config.yaml", False, True)


class TestPlatformURLValidation:
    """Tests for platform URL validation during wizard."""

    def test_init_github_enterprise_rejects_invalid_url(self, runner, tmp_path):
        """Test GitHub Enterprise URL validation rejects invalid URLs."""
        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Try GitHub Enterprise with invalid URL, then valid URL
            # Invalid: "not-a-url" (missing protocol)
            # Valid: https://github.example.com/api/v3
            inputs = (
                "1\ngithub\ny\nnot-a-url\n"
                "https://github.example.com/api/v3\n"
                "owner/*\nn\ny\nn\nn\nn\nn\n"
            )
            result = runner.invoke(cli, ["init"], input=inputs)

            # Should succeed after retry
            assert result.exit_code == 0
            # Verify error message appeared
            assert "invalid" in result.output.lower() or "error" in result.output.lower()
            # Verify config created with valid URL
            config_content = Path("config.yaml").read_text()
            assert "https://github.example.com/api/v3" in config_content

    def test_init_gitea_rejects_invalid_url(self, runner, tmp_path):
        """Test Gitea URL validation rejects malformed URLs."""
        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Try Gitea with invalid URL, then valid URL
            # Invalid: "gitea-server" (missing protocol)
            # Valid: http://192.168.1.14:3000
            inputs = "1\ngitea\ngitea-server\nhttp://192.168.1.14:3000\n\nn\ny\nn\nn\nn\nn\n"
            result = runner.invoke(cli, ["init"], input=inputs)

            # Should succeed after retry
            assert result.exit_code == 0
            # Verify error message appeared
            assert "invalid" in result.output.lower() or "error" in result.output.lower()
            # Verify config created with valid URL
            config_content = Path("config.yaml").read_text()
            assert "http://192.168.1.14:3000" in config_content

    def test_init_gitlab_selfhosted_rejects_invalid_url(self, runner, tmp_path):
        """Test GitLab self-hosted URL validation rejects invalid URLs."""
        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Try GitLab self-hosted with invalid URL, then valid URL
            # Invalid: "my-gitlab" (missing protocol)
            # Valid: https://gitlab.internal.company.com
            inputs = (
                "1\ngitlab\ny\nmy-gitlab\nhttps://gitlab.internal.company.com\n\nn\ny\nn\nn\nn\nn\n"
            )
            result = runner.invoke(cli, ["init"], input=inputs)

            # Should succeed after retry
            assert result.exit_code == 0
            # Verify error message appeared
            assert "invalid" in result.output.lower() or "error" in result.output.lower()
            # Verify config created with valid URL
            config_content = Path("config.yaml").read_text()
            assert "https://gitlab.internal.company.com" in config_content


class TestAdvancedSettingsBoundaries:
    """Tests for advanced LLM settings boundary validation."""

    def test_init_advanced_settings_temperature_too_high(self, runner, tmp_path):
        """Test temperature validation rejects values > 2.0."""
        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Try OpenAI with advanced settings, temperature too high
            # Invalid: 3.0 (max is 2.0)
            # Valid: 0.7
            inputs = (
                "1\ngitea\n\n\ny\nopenai-compatible\n\n\nn\ny\n"
                "3.0\n0.7\n"  # temp too high, then valid
                "\n\n\n\n\n"  # defaults: max_tokens, timeout, retries, concurrent, req/min
                "n\ny\nn\nn\nn\nn\n"  # cache, docs, markdown, custom_dict, db, env
            )
            result = runner.invoke(cli, ["init"], input=inputs)

            # Should succeed after retry
            assert result.exit_code == 0
            # Verify error message appeared (Click's FloatRange error)
            assert "not in the range" in result.output
            # Verify config created with valid temperature
            config_content = Path("config.yaml").read_text()
            assert "temperature: 0.7" in config_content

    def test_init_advanced_settings_temperature_too_low(self, runner, tmp_path):
        """Test temperature validation rejects values < 0.0."""
        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Try OpenAI with advanced settings, temperature too low
            # Invalid: -0.1 (min is 0.0)
            # Valid: 0.2
            inputs = (
                "1\ngitea\n\n\ny\nopenai-compatible\n\n\nn\ny\n"
                "-0.1\n0.2\n"  # temp too low, then valid
                "\n\n\n\n\n"  # defaults: max_tokens, timeout, retries, concurrent, req/min
                "n\ny\nn\nn\nn\nn\n"
            )
            result = runner.invoke(cli, ["init"], input=inputs)

            # Should succeed after retry
            assert result.exit_code == 0
            # Verify error message appeared (Click's FloatRange error)
            assert "not in the range" in result.output
            # Verify config created with valid temperature
            config_content = Path("config.yaml").read_text()
            assert "temperature: 0.2" in config_content

    def test_init_advanced_settings_max_tokens_negative(self, runner, tmp_path):
        """Test max_tokens validation rejects negative values."""
        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Try OpenAI with advanced settings, max_tokens negative
            # Invalid: -100 (min is 100)
            # Valid: 8000
            inputs = (
                "1\ngitea\n\n\ny\nopenai-compatible\n\n\nn\ny\n"
                "\n"  # temp default
                "-100\n8000\n"  # max_tokens negative, then valid
                "\n\n\n\n"  # defaults: timeout, retries, concurrent, req/min
                "n\ny\nn\nn\nn\nn\n"
            )
            result = runner.invoke(cli, ["init"], input=inputs)

            # Should succeed after retry
            assert result.exit_code == 0
            # Verify error message appeared (Click's IntRange error)
            assert "not in the range" in result.output
            # Verify config created with valid max_tokens
            config_content = Path("config.yaml").read_text()
            assert "max_tokens: 8000" in config_content

    def test_init_advanced_settings_max_tokens_too_large(self, runner, tmp_path):
        """Test max_tokens validation rejects values > 20000."""
        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Try OpenAI with advanced settings, max_tokens too large
            # Invalid: 25000 (max is 20000)
            # Valid: 16000
            inputs = (
                "1\ngitea\n\n\ny\nopenai-compatible\n\n\nn\ny\n"
                "\n"  # temp default
                "25000\n16000\n"  # max_tokens too large, then valid
                "\n\n\n\n"  # defaults: timeout, retries, concurrent, req/min
                "n\ny\nn\nn\nn\nn\n"
            )
            result = runner.invoke(cli, ["init"], input=inputs)

            # Should succeed after retry
            assert result.exit_code == 0
            # Verify error message appeared (Click's IntRange error)
            assert "not in the range" in result.output
            # Verify config created with valid max_tokens
            config_content = Path("config.yaml").read_text()
            assert "max_tokens: 16000" in config_content
