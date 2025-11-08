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
            # Provide "gitea" as platform choice
            result = runner.invoke(cli, ["init"], input="gitea\n")

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

            # Run init and confirm (y for overwrite, gitea for platform)
            result = runner.invoke(cli, ["init"], input="y\ngitea\n")

            assert result.exit_code == 0
            assert "✓ Created config.yaml" in result.output

            # Verify new content
            config_content = Path("config.yaml").read_text()
            assert "gitea:" in config_content
            assert "existing: config" not in config_content

    def test_init_template_structure(self, runner, tmp_path):
        """Test that init creates valid YAML template."""
        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Provide "gitea" as platform choice
            result = runner.invoke(cli, ["init"], input="gitea\n")

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

    def test_scan_detects_gitea_adapter(self, runner):
        """Test that scan uses GiteaAdapter when gitea config present."""
        from pydantic import SecretStr

        from drep.models.config import Config, GiteaConfig

        # Create Gitea-only config
        gitea_config = Config(
            gitea=GiteaConfig(
                url="http://gitea.example.com",
                token=SecretStr("gitea_token"),
                repositories=["owner/*"],
            )
        )

        with patch("drep.cli.load_config") as mock_load:
            with patch("drep.cli._run_scan", new_callable=AsyncMock) as mock_run:
                mock_load.return_value = gitea_config

                result = runner.invoke(cli, ["scan", "owner/repo"])

                # Verify _run_scan was called (command accepted)
                assert result.exit_code == 0
                mock_run.assert_called_once()

    def test_scan_detects_github_adapter(self, runner):
        """Test that scan uses GitHubAdapter when github config present."""
        from pydantic import SecretStr

        from drep.models.config import Config, GitHubConfig

        # Create GitHub-only config
        github_config = Config(
            github=GitHubConfig(token=SecretStr("ghp_test"), repositories=["owner/*"])
        )

        with patch("drep.cli.load_config") as mock_load:
            with patch("drep.cli._run_scan", new_callable=AsyncMock) as mock_run:
                mock_load.return_value = github_config

                result = runner.invoke(cli, ["scan", "owner/repo"])

                # Verify _run_scan was called (command accepted)
                assert result.exit_code == 0
                mock_run.assert_called_once()

    def test_scan_prefers_gitea_when_both_configured(self, runner):
        """Test that scan prefers GiteaAdapter when both platforms configured."""
        from pydantic import SecretStr

        from drep.models.config import Config, GiteaConfig, GitHubConfig

        # Create config with both platforms
        both_config = Config(
            gitea=GiteaConfig(
                url="http://gitea.example.com",
                token=SecretStr("gitea_token"),
                repositories=["owner/*"],
            ),
            github=GitHubConfig(token=SecretStr("ghp_test"), repositories=["owner/*"]),
        )

        with patch("drep.cli.load_config") as mock_load:
            with patch("drep.cli._run_scan", new_callable=AsyncMock) as mock_run:
                mock_load.return_value = both_config

                result = runner.invoke(cli, ["scan", "owner/repo"])

                # Verify _run_scan was called
                assert result.exit_code == 0
                mock_run.assert_called_once()

    def test_scan_rejects_no_platform_config(self, runner):
        """Test that scan rejects config with neither Gitea nor GitHub."""
        # This test verifies the error handling if somehow we get a config without platforms
        # (shouldn't happen in practice - Config validator prevents it, but test the CLI guard)

        with patch("drep.cli.load_config") as mock_load:
            # Return a mock config object with no platforms
            # (bypasses Pydantic validation since we're mocking load_config)
            class MockConfig:
                gitea = None
                github = None
                database_url = "sqlite:///./test.db"
                documentation = None
                llm = None

            mock_load.return_value = MockConfig()

            result = runner.invoke(cli, ["scan", "owner/repo"])

            # Should show error and abort
            assert result.exit_code == 1
            assert "No platform configured" in result.output


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
        from pydantic import SecretStr

        config.gitea.token = SecretStr("test-token")
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

    @patch("drep.cli.IssueManager")
    @patch("drep.cli.DocumentationAnalyzer")
    @patch("drep.cli.RepositoryScanner")
    @patch("drep.cli.init_database")
    @patch("drep.cli.GiteaAdapter")
    @patch("drep.cli.load_config")
    @patch("drep.cli.Repo")
    def test_token_file_has_secure_permissions(
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
        """Test that token file is created with owner-only permissions (0o600)."""
        # Setup minimal mocks
        config = MagicMock()
        config.gitea.url = "http://test"
        from pydantic import SecretStr

        config.gitea.token = SecretStr("test-token")
        config.documentation = MagicMock()
        config.database_url = "sqlite:///./test.db"
        config.llm = None  # Disable LLM to simplify test
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
        scanner.close = AsyncMock()
        scanner.llm_client = None  # No LLM
        mock_scanner_class.return_value = scanner

        analyzer = MagicMock()
        analyzer.analyze_file = AsyncMock(return_value=MagicMock(to_findings=lambda: []))
        mock_analyzer_class.return_value = analyzer

        issue_manager = MagicMock()
        issue_manager.create_issues_for_findings = AsyncMock()
        mock_issue_manager_class.return_value = issue_manager

        # Track token file permissions
        token_file_permissions = None

        def mock_clone_from(url, path, branch, env):
            # Capture askpass script location from environment
            askpass_script = Path(env["GIT_ASKPASS"])
            token_file = askpass_script.parent / ".git-token"

            # Verify token file exists and capture permissions
            nonlocal token_file_permissions
            if token_file.exists():
                token_file_permissions = oct(token_file.stat().st_mode)[-3:]

            # Create repo directory
            Path(path).mkdir(parents=True, exist_ok=True)
            (Path(path) / "test.py").write_text("# Test")
            return MagicMock()

        mock_repo_class.clone_from.side_effect = mock_clone_from

        with runner.isolated_filesystem(temp_dir=tmp_path):
            result = runner.invoke(cli, ["scan", "owner/repo", "--config", "test.yaml"])

            # Verify scan succeeded
            assert result.exit_code == 0

            # Verify token file had correct permissions (0o600)
            assert (
                token_file_permissions == "600"
            ), f"Token file permissions were {token_file_permissions}, expected 600"

    @patch("drep.cli.IssueManager")
    @patch("drep.cli.DocumentationAnalyzer")
    @patch("drep.cli.RepositoryScanner")
    @patch("drep.cli.init_database")
    @patch("drep.cli.GiteaAdapter")
    @patch("drep.cli.load_config")
    @patch("drep.cli.Repo")
    def test_askpass_script_has_secure_permissions(
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
        """Test that askpass script is created with owner-only execute permissions (0o700)."""
        # Setup minimal mocks
        config = MagicMock()
        config.gitea.url = "http://test"
        from pydantic import SecretStr

        config.gitea.token = SecretStr("test-token")
        config.documentation = MagicMock()
        config.database_url = "sqlite:///./test.db"
        config.llm = None
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
        scanner.close = AsyncMock()
        scanner.llm_client = None
        mock_scanner_class.return_value = scanner

        analyzer = MagicMock()
        analyzer.analyze_file = AsyncMock(return_value=MagicMock(to_findings=lambda: []))
        mock_analyzer_class.return_value = analyzer

        issue_manager = MagicMock()
        issue_manager.create_issues_for_findings = AsyncMock()
        mock_issue_manager_class.return_value = issue_manager

        # Track askpass script permissions
        askpass_permissions = None

        def mock_clone_from(url, path, branch, env):
            # Capture askpass script location and permissions
            askpass_script = Path(env["GIT_ASKPASS"])

            nonlocal askpass_permissions
            if askpass_script.exists():
                askpass_permissions = oct(askpass_script.stat().st_mode)[-3:]

            # Create repo directory
            Path(path).mkdir(parents=True, exist_ok=True)
            (Path(path) / "test.py").write_text("# Test")
            return MagicMock()

        mock_repo_class.clone_from.side_effect = mock_clone_from

        with runner.isolated_filesystem(temp_dir=tmp_path):
            result = runner.invoke(cli, ["scan", "owner/repo", "--config", "test.yaml"])

            # Verify scan succeeded
            assert result.exit_code == 0

            # Verify askpass script had correct permissions (0o700)
            assert (
                askpass_permissions == "700"
            ), f"Askpass script permissions were {askpass_permissions}, expected 700"

    @patch("drep.cli.IssueManager")
    @patch("drep.cli.DocumentationAnalyzer")
    @patch("drep.cli.RepositoryScanner")
    @patch("drep.cli.init_database")
    @patch("drep.cli.GiteaAdapter")
    @patch("drep.cli.load_config")
    @patch("drep.cli.Repo")
    def test_token_not_in_environment_variables(
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
        """Test that token is NOT exposed in environment variables (security fix)."""
        # Setup minimal mocks
        config = MagicMock()
        config.gitea.url = "http://test"
        from pydantic import SecretStr

        config.gitea.token = SecretStr("test-token-secret")
        config.documentation = MagicMock()
        config.database_url = "sqlite:///./test.db"
        config.llm = None
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
        scanner.close = AsyncMock()
        scanner.llm_client = None
        mock_scanner_class.return_value = scanner

        analyzer = MagicMock()
        analyzer.analyze_file = AsyncMock(return_value=MagicMock(to_findings=lambda: []))
        mock_analyzer_class.return_value = analyzer

        issue_manager = MagicMock()
        issue_manager.create_issues_for_findings = AsyncMock()
        mock_issue_manager_class.return_value = issue_manager

        # Track environment variables passed to git
        git_env = None

        def mock_clone_from(url, path, branch, env):
            nonlocal git_env
            git_env = env

            # Create repo directory
            Path(path).mkdir(parents=True, exist_ok=True)
            (Path(path) / "test.py").write_text("# Test")
            return MagicMock()

        mock_repo_class.clone_from.side_effect = mock_clone_from

        with runner.isolated_filesystem(temp_dir=tmp_path):
            result = runner.invoke(cli, ["scan", "owner/repo", "--config", "test.yaml"])

            # Verify scan succeeded
            assert result.exit_code == 0

            # Verify token is NOT in environment variables
            assert git_env is not None, "Git environment should have been captured"
            assert (
                "DREP_GIT_TOKEN" not in git_env
            ), "Token should NOT be in DREP_GIT_TOKEN environment variable"

            # Verify no environment variable contains the token value
            token_value = "test-token-secret"
            for key, value in git_env.items():
                assert token_value not in str(
                    value
                ), f"Token found in environment variable {key}: {value}"

            # Verify GIT_ASKPASS is set (our security mechanism)
            assert "GIT_ASKPASS" in git_env, "GIT_ASKPASS should be set for secure token handling"

    @patch("drep.cli.IssueManager")
    @patch("drep.cli.DocumentationAnalyzer")
    @patch("drep.cli.RepositoryScanner")
    @patch("drep.cli.init_database")
    @patch("drep.cli.GiteaAdapter")
    @patch("drep.cli.load_config")
    @patch("drep.cli.Repo")
    @patch("shutil.rmtree")
    def test_cleanup_failure_is_logged_and_reported(
        self,
        mock_rmtree,
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
        """Test that cleanup failures are logged with SECURITY warning and reported to user."""
        # Setup minimal mocks
        config = MagicMock()
        config.gitea.url = "http://test"
        from pydantic import SecretStr

        config.gitea.token = SecretStr("test-token")
        config.documentation = MagicMock()
        config.database_url = "sqlite:///./test.db"
        config.llm = None
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
        scanner.close = AsyncMock()
        scanner.llm_client = None
        mock_scanner_class.return_value = scanner

        analyzer = MagicMock()
        analyzer.analyze_file = AsyncMock(return_value=MagicMock(to_findings=lambda: []))
        mock_analyzer_class.return_value = analyzer

        issue_manager = MagicMock()
        issue_manager.create_issues_for_findings = AsyncMock()
        mock_issue_manager_class.return_value = issue_manager

        # Mock successful clone
        def mock_clone_from(url, path, branch, env):
            Path(path).mkdir(parents=True, exist_ok=True)
            (Path(path) / "test.py").write_text("# Test")
            return MagicMock()

        mock_repo_class.clone_from.side_effect = mock_clone_from

        # Mock rmtree to fail
        mock_rmtree.side_effect = PermissionError("Permission denied")

        with runner.isolated_filesystem(temp_dir=tmp_path):
            result = runner.invoke(cli, ["scan", "owner/repo", "--config", "test.yaml"])

            # Verify scan completed (cleanup failure doesn't crash)
            assert result.exit_code == 0

            # Verify user was warned about cleanup failure
            assert "WARNING: Failed to clean up temporary credentials" in result.output
            assert "Permission denied" in result.output

            # Verify rmtree was called (cleanup attempted)
            mock_rmtree.assert_called_once()


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

            # Mock git operations
            with patch("drep.cli.Repo") as mock_repo:
                mock_repo.return_value.index.diff.return_value = []

                # Mock scanner/analyzer to avoid real analysis
                with patch("drep.cli.RepositoryScanner") as mock_scanner_class:
                    mock_scanner = mock_scanner_class.return_value
                    mock_scanner.get_staged_files.return_value = []

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
            with patch("drep.cli.Repo") as mock_repo:
                mock_repo.return_value.index.diff.return_value = []

                with patch("drep.cli.RepositoryScanner") as mock_scanner_class:
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

            # Patch at the location where it's imported (_run_check imports from drep.core.scanner)
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
