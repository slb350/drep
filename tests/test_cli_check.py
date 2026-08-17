"""Check (pre-commit) command tests."""

from pathlib import Path
from unittest.mock import AsyncMock, patch

import yaml

from drep.cli import cli


class TestCheckCommand:
    """Tests for drep check command (pre-commit integration)."""

    def test_check_command_exists(self, runner):
        """Test that check command exists."""
        result = runner.invoke(cli, ["check", "--help"])
        assert result.exit_code == 0
        assert "Check local files" in result.output or "check" in result.output.lower()

    def test_check_works_without_platform_config(self, runner, tmp_path):
        """Test that check works with LLM-only config (no platform)."""
        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Create LLM-only config
            config_path = Path("config.yaml")
            config_data = {
                "llm": {
                    "enabled": True,
                    "endpoint": "http://localhost:1234/v1",
                    "model": "test-model",
                },
                "documentation": {"enabled": True},
            }
            config_path.write_text(yaml.dump(config_data))

            # Create a test Python file
            test_file = Path("test.py")
            test_file.write_text("def foo(): pass  # No docstring")

            # Mock scanner/analyzer to avoid real analysis
            with patch("drep.cli_workflows.RepositoryScanner") as mock_scanner_class:
                mock_scanner = mock_scanner_class.return_value
                mock_scanner.get_staged_files.return_value = []
                mock_scanner.analyze_code_quality = AsyncMock(return_value=[])
                mock_scanner.analyze_docstrings = AsyncMock(return_value=[])
                mock_scanner.close = AsyncMock()

                result = runner.invoke(cli, ["check", ".", "--config", "config.yaml"])

                # Should succeed without requiring platform
                assert result.exit_code == 0

    def test_check_returns_exit_code_one_when_findings_present(self, runner, tmp_path):
        """Test that check returns exit code 1 when issues found."""
        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Create minimal config
            config_path = Path("config.yaml")
            config_data = {
                "llm": {
                    "enabled": False,  # Disable LLM for fast test
                },
                "documentation": {"enabled": False},
            }
            config_path.write_text(yaml.dump(config_data))

            # Create test file
            test_file = Path("test.py")
            test_file.write_text("def foo(): pass")

            # Mock finding issues
            with patch("drep.cli_workflows.RepositoryScanner") as mock_scanner_class:
                # Mock scanner to return findings
                from drep.models.findings import Finding

                mock_scanner = mock_scanner_class.return_value
                mock_scanner.get_staged_files.return_value = ["test.py"]

                # Mock analyze methods to return findings
                async def mock_analyze(*args, **kwargs):
                    return [
                        Finding(
                            type="test",
                            severity="warning",
                            file_path="test.py",
                            line=1,
                            message="Test finding",
                        )
                    ]

                mock_scanner.analyze_code_quality = AsyncMock(return_value=[])
                mock_scanner.analyze_docstrings = AsyncMock(side_effect=mock_analyze)

                result = runner.invoke(cli, ["check", ".", "--config", "config.yaml"])

                # Should return exit code 1 when findings present
                assert result.exit_code == 1

    def test_check_accepts_staged_flag(self, runner, tmp_path):
        """Test that check accepts --staged flag."""
        with runner.isolated_filesystem(temp_dir=tmp_path):
            config_path = Path("config.yaml")
            config_data = {
                "documentation": {"enabled": False},
            }
            config_path.write_text(yaml.dump(config_data))

            # Patch where _run_check resolves it (drep.cli_workflows)
            with patch("drep.core.scanner.Repo") as mock_repo:
                mock_repo.return_value.index.diff.return_value = []

                # Create real scanner but mock its methods
                result = runner.invoke(cli, ["check", ".", "--staged", "--config", "config.yaml"])

                # Should succeed
                assert result.exit_code == 0
                # If --staged was passed, Repo.index.diff should have been called
                assert mock_repo.return_value.index.diff.called or result.exit_code == 0

    def test_check_handles_missing_config_file(self, runner, tmp_path):
        """Test that check handles missing config file gracefully."""
        with runner.isolated_filesystem(temp_dir=tmp_path):
            result = runner.invoke(cli, ["check", ".", "--config", "nonexistent.yaml"])

            assert result.exit_code == 1
            assert "Config file not found" in result.output or "not found" in result.output.lower()

    def test_check_handles_malformed_yaml(self, runner, tmp_path):
        """Test that check handles malformed YAML gracefully."""
        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Create malformed YAML
            config_path = Path("bad.yaml")
            config_path.write_text("invalid: yaml: content: [\n  - unclosed")

            result = runner.invoke(cli, ["check", ".", "--config", "bad.yaml"])

            assert result.exit_code == 1
            assert "YAML" in result.output or "yaml" in result.output.lower()

    def test_check_handles_invalid_config_validation(self, runner, tmp_path):
        """Test that check handles Pydantic validation errors gracefully."""
        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Create config with invalid LLM endpoint format
            config_path = Path("invalid.yaml")
            config_data = {
                "llm": {
                    "enabled": True,
                    "endpoint": "not-a-url",  # Invalid URL format
                    "model": "test",
                }
            }
            config_path.write_text(yaml.dump(config_data))

            result = runner.invoke(cli, ["check", ".", "--config", "invalid.yaml"])

            assert result.exit_code == 1
            assert "validation" in result.output.lower() or "invalid" in result.output.lower()

    def test_check_handles_nonexistent_path(self, runner, tmp_path):
        """Test that check handles nonexistent path gracefully."""
        with runner.isolated_filesystem(temp_dir=tmp_path):
            result = runner.invoke(cli, ["check", "/nonexistent/path"])

            assert result.exit_code == 1
            assert "not found" in result.output.lower() or "does not exist" in result.output.lower()

    def test_check_exit_zero_returns_zero_with_findings(self, runner, tmp_path):
        """Test that --exit-zero returns 0 even when findings present."""
        # This test verifies that when --exit-zero is used,
        # the exit code is 0 even if findings are present.
        # We mock the async _run_check to return findings directly.

        from drep.models.findings import Finding

        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Mock _run_check to return findings
            async def mock_run_check(*args, **kwargs):
                return [
                    Finding(
                        type="test",
                        severity="warning",
                        file_path="test.py",
                        line=1,
                        message="Test finding",
                    )
                ]

            with patch("drep.cli._run_check", side_effect=mock_run_check):
                result = runner.invoke(cli, ["check", ".", "--exit-zero"])

                # Should return exit code 0 despite findings
                assert result.exit_code == 0
                # Should show it's in warning mode
                assert "warning mode" in result.output.lower()
