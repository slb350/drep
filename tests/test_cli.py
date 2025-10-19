"""Tests for CLI commands."""

from pathlib import Path
from unittest.mock import AsyncMock, MagicMock, patch

import pytest
import yaml
from click.testing import CliRunner

from drep.cli import cli


@pytest.fixture
def runner():
    """Create CLI test runner."""
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


class TestInitCommand:
    """Tests for drep init command."""

    def test_init_creates_config_file(self, runner, tmp_path):
        """Test that init command creates config.yaml."""
        # Run in temp directory
        with runner.isolated_filesystem(temp_dir=tmp_path):
            result = runner.invoke(cli, ["init"])

            assert result.exit_code == 0
            assert "✓ Created config.yaml" in result.output
            assert Path("config.yaml").exists()

            # Check content
            config_content = Path("config.yaml").read_text()
            assert "gitea:" in config_content
            assert "${GITEA_TOKEN}" in config_content
            assert "documentation:" in config_content

    def test_init_prompts_on_existing_file(self, runner, tmp_path):
        """Test that init prompts before overwriting existing file."""
        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Create existing config
            Path("config.yaml").write_text("existing: config")

            # Run init and abort
            result = runner.invoke(cli, ["init"], input="n\n")

            assert result.exit_code == 1
            assert "already exists" in result.output

            # Verify original file unchanged
            assert Path("config.yaml").read_text() == "existing: config"

    def test_init_overwrites_with_confirmation(self, runner, tmp_path):
        """Test that init overwrites file when user confirms."""
        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Create existing config
            Path("config.yaml").write_text("existing: config")

            # Run init and confirm
            result = runner.invoke(cli, ["init"], input="y\n")

            assert result.exit_code == 0
            assert "✓ Created config.yaml" in result.output

            # Verify new content
            config_content = Path("config.yaml").read_text()
            assert "gitea:" in config_content
            assert "existing: config" not in config_content

    def test_init_template_structure(self, runner, tmp_path):
        """Test that init creates valid YAML template."""
        with runner.isolated_filesystem(temp_dir=tmp_path):
            result = runner.invoke(cli, ["init"])

            assert result.exit_code == 0

            # Parse and validate YAML
            config = yaml.safe_load(Path("config.yaml").read_text())

            assert "gitea" in config
            assert "url" in config["gitea"]
            assert "token" in config["gitea"]
            assert "repositories" in config["gitea"]
            assert "documentation" in config
            assert "enabled" in config["documentation"]
            assert "custom_dictionary" in config["documentation"]
            assert "database_url" in config


class TestScanCommand:
    """Tests for drep scan command."""

    def test_scan_rejects_invalid_repository_format(self, runner):
        """Test that scan rejects repository without owner/repo format."""
        result = runner.invoke(cli, ["scan", "invalid-repo"])

        assert result.exit_code == 0  # Click doesn't exit non-zero by default
        assert "Error: Repository must be in format 'owner/repo'" in result.output

    def test_scan_accepts_valid_repository_format(self, runner, temp_config_file):
        """Test that scan accepts valid owner/repo format."""
        with patch("drep.cli._run_scan", new_callable=AsyncMock) as mock_scan:
            result = runner.invoke(cli, ["scan", "steve/drep", "--config", str(temp_config_file)])

            assert result.exit_code == 0
            assert "Scanning steve/drep" in result.output
            mock_scan.assert_called_once_with("steve", "drep", str(temp_config_file), False, True)

    def test_scan_uses_default_config_path(self, runner, tmp_path):
        """Test that scan uses default config.yaml path."""
        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Create default config
            Path("config.yaml").write_text(
                yaml.dump(
                    {
                        "gitea": {"url": "http://test", "token": "test"},
                        "documentation": {"enabled": True},
                        "database_url": "sqlite:///./test.db",
                    }
                )
            )

            with patch("drep.cli._run_scan", new_callable=AsyncMock) as mock_scan:
                result = runner.invoke(cli, ["scan", "owner/repo"])

                assert result.exit_code == 0
                mock_scan.assert_called_once_with("owner", "repo", "config.yaml", False, True)

    def test_scan_respects_config_option(self, runner, temp_config_file):
        """Test that scan respects --config option."""
        with patch("drep.cli._run_scan", new_callable=AsyncMock) as mock_scan:
            result = runner.invoke(cli, ["scan", "owner/repo", "--config", str(temp_config_file)])

            assert result.exit_code == 0
            mock_scan.assert_called_once_with("owner", "repo", str(temp_config_file), False, True)

    def test_scan_handles_missing_config_file(self, runner):
        """Test that scan shows helpful error when config file missing."""
        with patch("drep.cli.load_config") as mock_load:
            mock_load.side_effect = FileNotFoundError("Config not found")

            result = runner.invoke(cli, ["scan", "owner/repo", "--config", "missing.yaml"])

            assert result.exit_code == 0
            assert "Config file not found" in result.output
            assert "drep init" in result.output


class TestScanWorkflow:
    """Tests for scan workflow integration."""

    @patch("drep.cli.IssueManager")
    @patch("drep.cli.DocumentationAnalyzer")
    @patch("drep.cli.RepositoryScanner")
    @patch("drep.cli.init_database")
    @patch("drep.cli.GiteaAdapter")
    @patch("drep.cli.load_config")
    @patch("drep.cli.Repo")
    def test_successful_scan_workflow(
        self,
        mock_repo_class,
        mock_load_config,
        mock_adapter_class,
        mock_init_db,
        mock_scanner_class,
        mock_analyzer_class,
        mock_issue_manager_class,
        runner,
        tmp_path,
    ):
        """Test complete scan workflow with all components."""
        # Setup mocks
        config = MagicMock()
        config.gitea.url = "http://test"
        config.gitea.token = "test-token"
        config.documentation = MagicMock()
        config.database_url = "sqlite:///./test.db"
        mock_load_config.return_value = config

        adapter = AsyncMock()
        adapter.get_default_branch = AsyncMock(return_value="main")
        adapter.close = AsyncMock()
        mock_adapter_class.return_value = adapter

        session = MagicMock()
        mock_init_db.return_value = session

        scanner = MagicMock()
        scanner.scan_repository = AsyncMock(return_value=(["test.py"], "abc123"))
        scanner.record_scan = MagicMock()
        # Mock the LLM-powered analysis methods
        mock_finding = MagicMock()
        scanner.analyze_code_quality = AsyncMock(return_value=[mock_finding])
        scanner.analyze_docstrings = AsyncMock(return_value=[])
        scanner.close = AsyncMock()
        mock_scanner_class.return_value = scanner

        analyzer = MagicMock()
        analyzer.analyze_file = AsyncMock(return_value=MagicMock(to_findings=lambda: []))
        mock_analyzer_class.return_value = analyzer

        issue_manager = MagicMock()
        issue_manager.create_issues_for_findings = AsyncMock()
        mock_issue_manager_class.return_value = issue_manager

        # Mock git operations
        mock_repo = MagicMock()
        mock_repo_class.clone_from.return_value = mock_repo

        # Use isolated filesystem for test
        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Mock creating a clone - this will happen during clone_from
            # We need to create the file AFTER clone_from is called
            def mock_clone_from(url, path, branch, env):
                # Simulate successful clone by creating directory
                Path(path).mkdir(parents=True, exist_ok=True)
                test_file = Path(path) / "test.py"
                test_file.write_text("# Test file")
                return mock_repo

            mock_repo_class.clone_from.side_effect = mock_clone_from

            result = runner.invoke(cli, ["scan", "owner/repo", "--config", "test.yaml"])

            # Verify workflow
            assert result.exit_code == 0, f"Exit code: {result.exit_code}, Output: {result.output}"
            mock_load_config.assert_called_once()
            adapter.get_default_branch.assert_called_once_with("owner", "repo")
            mock_repo_class.clone_from.assert_called_once()
            scanner.scan_repository.assert_called_once()
            scanner.analyze_code_quality.assert_called_once()
            scanner.analyze_docstrings.assert_called_once()
            issue_manager.create_issues_for_findings.assert_called_once()
            scanner.record_scan.assert_called_once_with("owner", "repo", "abc123")
            scanner.close.assert_called_once()
            adapter.close.assert_called_once()
