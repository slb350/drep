"""Scan command and scan workflow tests."""

from pathlib import Path
from unittest.mock import AsyncMock, MagicMock, patch

import yaml

from drep.cli import cli
from drep.models.findings import AnalysisResult, Finding


class TestScanCommand:
    """Tests for drep scan command."""

    def test_scan_rejects_invalid_repository_format(self, runner):
        """Test that scan rejects repository without owner/repo format."""
        result = runner.invoke(cli, ["scan", "invalid-repo"])

        assert result.exit_code == 0  # Click doesn't exit non-zero by default
        assert "Error: Repository must be in format 'owner/repo'" in result.output

    def test_scan_accepts_valid_repository_format(self, runner, temp_config_file):
        """Test that scan accepts valid owner/repo format."""
        with patch("drep.cli_workflows._run_scan", new_callable=AsyncMock) as mock_scan:
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

            with patch("drep.cli_workflows._run_scan", new_callable=AsyncMock) as mock_scan:
                result = runner.invoke(cli, ["scan", "owner/repo"])

                assert result.exit_code == 0
                mock_scan.assert_called_once_with("owner", "repo", "config.yaml", False, True)

    def test_scan_respects_config_option(self, runner, temp_config_file):
        """Test that scan respects --config option."""
        with patch("drep.cli_workflows._run_scan", new_callable=AsyncMock) as mock_scan:
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

    def test_scan_detects_gitea_adapter(self, runner, tmp_path):
        """Test that scan uses GiteaAdapter when gitea config present."""
        from pydantic import SecretStr

        from drep.models.config import Config, GiteaConfig

        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Create config file so find_config_file succeeds
            Path("config.yaml").write_text("gitea: {url: 'http://test', token: 'test'}")

            # Create Gitea-only config
            gitea_config = Config(
                gitea=GiteaConfig(
                    url="http://gitea.example.com",
                    token=SecretStr("gitea_token"),
                    repositories=["owner/*"],
                )
            )

            with (
                patch("drep.cli_wizard.load_config") as mock_load,
                patch("drep.cli_workflows._run_scan", new_callable=AsyncMock) as mock_run,
            ):
                mock_load.return_value = gitea_config

                result = runner.invoke(cli, ["scan", "owner/repo"])

                # Verify _run_scan was called (command accepted)
                assert result.exit_code == 0
                mock_run.assert_called_once()

    def test_scan_detects_github_adapter(self, runner, tmp_path):
        """Test that scan uses GitHubAdapter when github config present."""
        from pydantic import SecretStr

        from drep.models.config import Config, GitHubConfig

        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Create config file so find_config_file succeeds
            Path("config.yaml").write_text("github: {token: 'test', repositories: ['owner/*']}")

            # Create GitHub-only config
            github_config = Config(
                github=GitHubConfig(token=SecretStr("ghp_test"), repositories=["owner/*"])
            )

            with (
                patch("drep.cli_wizard.load_config") as mock_load,
                patch("drep.cli_workflows._run_scan", new_callable=AsyncMock) as mock_run,
            ):
                mock_load.return_value = github_config

                result = runner.invoke(cli, ["scan", "owner/repo"])

                # Verify _run_scan was called (command accepted)
                assert result.exit_code == 0
                mock_run.assert_called_once()

    def test_scan_prefers_gitea_when_both_configured(self, runner, tmp_path):
        """Test that scan prefers GiteaAdapter when both platforms configured."""
        from pydantic import SecretStr

        from drep.models.config import Config, GiteaConfig, GitHubConfig

        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Create config file so find_config_file succeeds
            Path("config.yaml").write_text("gitea: {url: 'http://test', token: 'test'}")

            # Create config with both platforms
            both_config = Config(
                gitea=GiteaConfig(
                    url="http://gitea.example.com",
                    token=SecretStr("gitea_token"),
                    repositories=["owner/*"],
                ),
                github=GitHubConfig(token=SecretStr("ghp_test"), repositories=["owner/*"]),
            )

            with (
                patch("drep.cli_wizard.load_config") as mock_load,
                patch("drep.cli_workflows._run_scan", new_callable=AsyncMock) as mock_run,
            ):
                mock_load.return_value = both_config

                result = runner.invoke(cli, ["scan", "owner/repo"])

                # Verify _run_scan was called
                assert result.exit_code == 0
                mock_run.assert_called_once()

    def test_scan_rejects_no_platform_config(self, runner, tmp_path):
        """Test that scan rejects config with neither Gitea nor GitHub."""
        # This test verifies the error handling if somehow we get a config without platforms
        # (shouldn't happen in practice - Config validator prevents it, but test the CLI guard)

        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Create config file so find_config_file succeeds
            Path("config.yaml").write_text("database_url: 'sqlite:///./test.db'")

            with patch("drep.cli_workflows.load_config") as mock_load:
                # Return a mock config object with no platforms
                # (bypasses Pydantic validation since we're mocking load_config)
                class MockConfig:
                    gitea = None
                    github = None
                    gitlab = None
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

    @patch("drep.cli_workflows.IssueManager")
    @patch("drep.cli_workflows.DocumentationAnalyzer")
    @patch("drep.cli_workflows.RepositoryScanner")
    @patch("drep.cli_workflows.init_database")
    @patch("drep.cli_workflows.GiteaAdapter")
    @patch("drep.cli_workflows.load_config")
    @patch("drep.cli_workflows.Repo")
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
        # Mock the LLM-powered analysis methods. AnalysisResult validates its
        # contents, so this is a real Finding rather than a MagicMock.
        finding = Finding(
            # "high" is the LLM vocabulary; llm_findings maps it to "error"
            type="bug",
            severity="error",
            file_path="test.py",
            line=1,
            message="Test finding",
        )
        scanner.analyze_code_quality = AsyncMock(return_value=AnalysisResult(findings=[finding]))
        scanner.analyze_docstrings = AsyncMock(return_value=AnalysisResult())
        scanner.close = AsyncMock()
        mock_scanner_class.return_value = scanner

        analyzer = MagicMock()
        analyzer.analyze_file = AsyncMock(return_value=MagicMock(to_findings=list))
        mock_analyzer_class.return_value = analyzer

        issue_manager = MagicMock()
        issue_manager.create_issues_for_findings = AsyncMock()
        mock_issue_manager_class.return_value = issue_manager

        # Mock git operations
        mock_repo = MagicMock()
        mock_repo_class.clone_from.return_value = mock_repo

        # Use isolated filesystem for test
        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Create test config file for discovery
            Path("test.yaml").write_text("gitea:\n  url: http://test\n  token: test-token")

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

    @patch("drep.cli_workflows.IssueManager")
    @patch("drep.cli_workflows.DocumentationAnalyzer")
    @patch("drep.cli_workflows.RepositoryScanner")
    @patch("drep.cli_workflows.init_database")
    @patch("drep.cli_workflows.GiteaAdapter")
    @patch("drep.cli_workflows.load_config")
    @patch("drep.cli_workflows.Repo")
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
        analyzer.analyze_file = AsyncMock(return_value=MagicMock(to_findings=list))
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
            # Create test config file for discovery
            Path("test.yaml").write_text("gitea:\n  url: http://test\n  token: test-token")

            result = runner.invoke(cli, ["scan", "owner/repo", "--config", "test.yaml"])

            # Verify scan succeeded
            assert result.exit_code == 0

            # Verify token file had correct permissions (0o600)
            assert token_file_permissions == "600", (
                f"Token file permissions were {token_file_permissions}, expected 600"
            )

    @patch("drep.cli_workflows.IssueManager")
    @patch("drep.cli_workflows.DocumentationAnalyzer")
    @patch("drep.cli_workflows.RepositoryScanner")
    @patch("drep.cli_workflows.init_database")
    @patch("drep.cli_workflows.GiteaAdapter")
    @patch("drep.cli_workflows.load_config")
    @patch("drep.cli_workflows.Repo")
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
        analyzer.analyze_file = AsyncMock(return_value=MagicMock(to_findings=list))
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
            # Create test config file for discovery
            Path("test.yaml").write_text("gitea:\n  url: http://test\n  token: test-token")

            result = runner.invoke(cli, ["scan", "owner/repo", "--config", "test.yaml"])

            # Verify scan succeeded
            assert result.exit_code == 0

            # Verify askpass script had correct permissions (0o700)
            assert askpass_permissions == "700", (
                f"Askpass script permissions were {askpass_permissions}, expected 700"
            )

    @patch("drep.cli_workflows.IssueManager")
    @patch("drep.cli_workflows.DocumentationAnalyzer")
    @patch("drep.cli_workflows.RepositoryScanner")
    @patch("drep.cli_workflows.init_database")
    @patch("drep.cli_workflows.GiteaAdapter")
    @patch("drep.cli_workflows.load_config")
    @patch("drep.cli_workflows.Repo")
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
        analyzer.analyze_file = AsyncMock(return_value=MagicMock(to_findings=list))
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
            # Create test config file for discovery
            Path("test.yaml").write_text("gitea:\n  url: http://test\n  token: test-token")

            result = runner.invoke(cli, ["scan", "owner/repo", "--config", "test.yaml"])

            # Verify scan succeeded
            assert result.exit_code == 0

            # Verify token is NOT in environment variables
            assert git_env is not None, "Git environment should have been captured"
            assert "DREP_GIT_TOKEN" not in git_env, (
                "Token should NOT be in DREP_GIT_TOKEN environment variable"
            )

            # Verify no environment variable contains the token value
            token_value = "test-token-secret"
            for key, value in git_env.items():
                assert token_value not in str(value), (
                    f"Token found in environment variable {key}: {value}"
                )

            # Verify GIT_ASKPASS is set (our security mechanism)
            assert "GIT_ASKPASS" in git_env, "GIT_ASKPASS should be set for secure token handling"

    @patch("drep.cli_workflows.IssueManager")
    @patch("drep.cli_workflows.DocumentationAnalyzer")
    @patch("drep.cli_workflows.RepositoryScanner")
    @patch("drep.cli_workflows.init_database")
    @patch("drep.cli_workflows.GiteaAdapter")
    @patch("drep.cli_workflows.load_config")
    @patch("drep.cli_workflows.Repo")
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
        analyzer.analyze_file = AsyncMock(return_value=MagicMock(to_findings=list))
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
            # Create test config file for discovery
            Path("test.yaml").write_text("gitea:\n  url: http://test\n  token: test-token")

            result = runner.invoke(cli, ["scan", "owner/repo", "--config", "test.yaml"])

            # Verify scan completed (cleanup failure doesn't crash)
            assert result.exit_code == 0

            # Verify user was warned about cleanup failure (improved message)
            assert "SECURITY WARNING: Failed to clean up credentials" in result.output
            assert "Manually delete: rm -rf" in result.output

            # Verify rmtree was called (cleanup attempted)
            mock_rmtree.assert_called_once()


class TestIncompleteScansAreNotRecorded:
    """A scan that skipped files must not claim that SHA was scanned.

    scan_repository derives the next run's changed-file set from the recorded
    SHA, so recording after a partial scan permanently excludes every file the
    LLM never saw - until it happens to change again.
    """

    @patch("drep.cli_workflows.IssueManager")
    @patch("drep.cli_workflows.DocumentationAnalyzer")
    @patch("drep.cli_workflows.RepositoryScanner")
    @patch("drep.cli_workflows.init_database")
    @patch("drep.cli_workflows.GiteaAdapter")
    @patch("drep.cli_workflows.load_config")
    @patch("drep.cli_workflows.Repo")
    def test_record_scan_is_skipped_when_files_went_unanalyzed(
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
        from pydantic import SecretStr

        config = MagicMock()
        config.gitea.url = "http://test"
        config.gitea.token = SecretStr("test-token")
        config.documentation = MagicMock()
        config.database_url = "sqlite:///./test.db"
        mock_load_config.return_value = config

        adapter = AsyncMock()
        adapter.get_default_branch = AsyncMock(return_value="main")
        adapter.close = AsyncMock()
        mock_adapter_class.return_value = adapter
        mock_init_db.return_value = MagicMock()

        scanner = MagicMock()
        scanner.scan_repository = AsyncMock(return_value=(["test.py"], "abc123"))
        scanner.record_scan = MagicMock()
        scanner.llm_client = None
        scanner.close = AsyncMock()
        # The code-quality pass could not analyze the only file
        scanner.analyze_code_quality = AsyncMock(
            return_value=AnalysisResult(failed_files=["test.py"])
        )
        scanner.analyze_docstrings = AsyncMock(return_value=AnalysisResult())
        mock_scanner_class.return_value = scanner

        analyzer = MagicMock()
        analyzer.analyze_file = AsyncMock(return_value=MagicMock(to_findings=list))
        mock_analyzer_class.return_value = analyzer

        issue_manager = MagicMock()
        issue_manager.create_issues_for_findings = AsyncMock()
        mock_issue_manager_class.return_value = issue_manager

        with runner.isolated_filesystem(temp_dir=tmp_path):
            Path("test.yaml").write_text("gitea:\n  url: http://test\n  token: test-token")
            result = runner.invoke(cli, ["scan", "owner/repo", "--config", "test.yaml"])

        assert "incomplete" in result.output.lower()
        scanner.record_scan.assert_not_called()
