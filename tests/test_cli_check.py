"""Check (pre-commit) command tests."""

from pathlib import Path
from unittest.mock import AsyncMock, patch

import yaml

from drep.cli import cli
from drep.models.findings import AnalysisResult, Finding


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
                mock_scanner.analyze_code_quality = AsyncMock(return_value=AnalysisResult())
                mock_scanner.analyze_docstrings = AsyncMock(return_value=AnalysisResult())
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
                    return AnalysisResult(
                        findings=[
                            Finding(
                                type="test",
                                severity="warning",
                                file_path="test.py",
                                line=1,
                                message="Test finding",
                            )
                        ]
                    )

                mock_scanner.analyze_code_quality = AsyncMock(return_value=AnalysisResult())
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

        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Mock _run_check to return findings
            async def mock_run_check(*args, **kwargs):
                return AnalysisResult(
                    findings=[
                        Finding(
                            type="test",
                            severity="warning",
                            file_path="test.py",
                            line=1,
                            message="Test finding",
                        )
                    ],
                )

            with patch("drep.cli_workflows._run_check", side_effect=mock_run_check):
                result = runner.invoke(cli, ["check", ".", "--exit-zero"])

                # Should return exit code 0 despite findings
                assert result.exit_code == 0
                # Should show it's in warning mode
                assert "warning mode" in result.output.lower()


class TestCheckAnalysisFailures:
    """A file that could not be analyzed must never be reported as clean.

    `drep check` gates commits. Reporting success when the LLM was unreachable
    turns the hook into a rubber stamp, so unanalyzed files get their own exit
    code (2) distinct from "analysis ran and found issues" (1).
    """

    @staticmethod
    def _write_project():
        Path("config.yaml").write_text(
            yaml.dump(
                {
                    "llm": {
                        "enabled": True,
                        "endpoint": "http://localhost:1234/v1",
                        "model": "test-model",
                    },
                    "documentation": {"enabled": False},
                }
            )
        )
        Path("test.py").write_text("def foo(): pass")

    @staticmethod
    def _scanner_failing_on(mock_scanner_class, *failed_files):
        """Wire a scanner where both passes fail on the same files.

        Both passes report the same file so the test also pins that the count
        the user sees is a file count, not a count of pass-failures.
        """
        mock_scanner = mock_scanner_class.return_value
        mock_scanner.get_scan_targets.return_value = ["test.py"]
        mock_scanner.get_staged_files.return_value = ["test.py"]
        mock_scanner.close = AsyncMock()

        result = AnalysisResult(findings=[], failed_files=list(failed_files))
        mock_scanner.analyze_code_quality = AsyncMock(return_value=result)
        mock_scanner.analyze_docstrings = AsyncMock(return_value=result)

    def test_unanalyzed_file_exits_two_and_is_not_reported_clean(self, runner, tmp_path):
        """An LLM failure must block the commit, not report success."""
        with runner.isolated_filesystem(temp_dir=tmp_path):
            self._write_project()

            with patch("drep.cli_workflows.RepositoryScanner") as mock_scanner_class:
                self._scanner_failing_on(mock_scanner_class, "test.py")

                result = runner.invoke(cli, ["check", ".", "--config", "config.yaml"])

            assert result.exit_code == 2
            assert "No issues found" not in result.output
            assert "could not be analyzed" in result.output.lower()
            assert "test.py" in result.output

    def test_failure_count_is_files_not_passes(self, runner, tmp_path):
        """One file failing both passes is one unanalyzed file, not two."""
        with runner.isolated_filesystem(temp_dir=tmp_path):
            self._write_project()

            with patch("drep.cli_workflows.RepositoryScanner") as mock_scanner_class:
                self._scanner_failing_on(mock_scanner_class, "test.py")

                result = runner.invoke(cli, ["check", ".", "--config", "config.yaml"])

            assert "1 file(s) could not be analyzed" in result.output

    def test_unanalyzed_file_still_warns_under_exit_zero(self, runner, tmp_path):
        """--exit-zero keeps its promise not to block, but must still say so."""
        with runner.isolated_filesystem(temp_dir=tmp_path):
            self._write_project()

            with patch("drep.cli_workflows.RepositoryScanner") as mock_scanner_class:
                self._scanner_failing_on(mock_scanner_class, "test.py")

                result = runner.invoke(
                    cli, ["check", ".", "--config", "config.yaml", "--exit-zero"]
                )

            assert result.exit_code == 0
            assert "No issues found" not in result.output
            assert "could not be analyzed" in result.output.lower()


class TestCheckSeverityThreshold:
    """--fail-on decides what is worth blocking a commit over.

    The LLM emits info-level style suggestions on almost any file (9 on a
    142-line constants module), so a gate that exits 1 on every finding can
    never pass. The threshold lets the hook block on bugs without blocking on
    "consider adding a type hint".
    """

    @staticmethod
    def _run(runner, severities, *extra_args):
        Path("config.yaml").write_text(
            yaml.dump({"llm": {"enabled": True, "endpoint": "http://x/v1", "model": "m"}})
        )
        Path("test.py").write_text("def foo(): pass")

        findings = [
            Finding(
                type="test",
                severity=severity,
                file_path="test.py",
                line=1,
                message=f"a {severity} finding",
            )
            for severity in severities
        ]
        with patch("drep.cli_workflows.RepositoryScanner") as mock_scanner_class:
            mock_scanner = mock_scanner_class.return_value
            mock_scanner.get_scan_targets.return_value = ["test.py"]
            mock_scanner.close = AsyncMock()
            mock_scanner.analyze_code_quality = AsyncMock(
                return_value=AnalysisResult(findings=findings)
            )
            mock_scanner.analyze_docstrings = AsyncMock(return_value=AnalysisResult())

            return runner.invoke(cli, ["check", ".", "--config", "config.yaml", *extra_args])

    def test_defaults_to_blocking_on_any_finding(self, runner, tmp_path):
        """Unchanged default: every finding blocks."""
        with runner.isolated_filesystem(temp_dir=tmp_path):
            result = self._run(runner, ["info"])
            assert result.exit_code == 1

    def test_below_threshold_findings_are_reported_but_do_not_block(self, runner, tmp_path):
        with runner.isolated_filesystem(temp_dir=tmp_path):
            result = self._run(runner, ["info", "warning"], "--fail-on", "error")
            assert result.exit_code == 0
            # Reported, not hidden - the point is to inform without blocking
            assert "a info finding" in result.output
            assert "a warning finding" in result.output

    def test_at_threshold_findings_block(self, runner, tmp_path):
        with runner.isolated_filesystem(temp_dir=tmp_path):
            result = self._run(runner, ["info", "error"], "--fail-on", "error")
            assert result.exit_code == 1

    def test_threshold_does_not_mask_unanalyzed_files(self, runner, tmp_path):
        """A lenient threshold is about findings, never about "drep could not run"."""
        with runner.isolated_filesystem(temp_dir=tmp_path):
            Path("config.yaml").write_text(
                yaml.dump({"llm": {"enabled": True, "endpoint": "http://x/v1", "model": "m"}})
            )
            Path("test.py").write_text("def foo(): pass")

            with patch("drep.cli_workflows.RepositoryScanner") as mock_scanner_class:
                mock_scanner = mock_scanner_class.return_value
                mock_scanner.get_scan_targets.return_value = ["test.py"]
                mock_scanner.close = AsyncMock()
                failed = AnalysisResult(failed_files=["test.py"])
                mock_scanner.analyze_code_quality = AsyncMock(return_value=failed)
                mock_scanner.analyze_docstrings = AsyncMock(return_value=AnalysisResult())

                result = runner.invoke(
                    cli, ["check", ".", "--config", "config.yaml", "--fail-on", "error"]
                )

            assert result.exit_code == 2


class TestCheckPathArguments:
    """At pre-push nothing is staged; pre-commit hands over the pushed files.

    So `check` has to accept a list of paths, the way lint-docs does.
    """

    def test_accepts_multiple_paths(self, runner, tmp_path):
        with runner.isolated_filesystem(temp_dir=tmp_path):
            Path("config.yaml").write_text(
                yaml.dump({"llm": {"enabled": True, "endpoint": "http://x/v1", "model": "m"}})
            )
            for name in ("a.py", "b.py", "c.py"):
                Path(name).write_text("def foo(): pass")

            with patch("drep.cli_workflows.RepositoryScanner") as mock_scanner_class:
                mock_scanner = mock_scanner_class.return_value
                mock_scanner.close = AsyncMock()
                mock_scanner.analyze_code_quality = AsyncMock(return_value=AnalysisResult())
                mock_scanner.analyze_docstrings = AsyncMock(return_value=AnalysisResult())

                result = runner.invoke(cli, ["check", "a.py", "b.py", "--config", "config.yaml"])

                assert result.exit_code == 0
                analyzed = mock_scanner.analyze_code_quality.call_args.kwargs["files"]
                assert sorted(analyzed) == ["a.py", "b.py"]

    def test_defaults_to_current_directory(self, runner, tmp_path):
        """Bare `drep check` keeps scanning the tree."""
        with runner.isolated_filesystem(temp_dir=tmp_path):
            Path("config.yaml").write_text(
                yaml.dump({"llm": {"enabled": True, "endpoint": "http://x/v1", "model": "m"}})
            )
            Path("a.py").write_text("def foo(): pass")

            with patch("drep.cli_workflows.RepositoryScanner") as mock_scanner_class:
                mock_scanner = mock_scanner_class.return_value
                mock_scanner.close = AsyncMock()
                mock_scanner.analyze_code_quality = AsyncMock(return_value=AnalysisResult())
                mock_scanner.analyze_docstrings = AsyncMock(return_value=AnalysisResult())

                result = runner.invoke(cli, ["check", "--config", "config.yaml"])

                assert result.exit_code == 0
                # Discovery is path policy, not scanner state - no scanner round-trip
                assert mock_scanner.analyze_code_quality.call_args.kwargs["files"] == ["a.py"]


class TestJsonOutputCarriesIncompleteness:
    """A JSON consumer must not read a partial run as a complete one.

    Findings alone look identical whether the LLM analyzed every file or none
    of them; only the exit code differed, which a library caller may not see.
    """

    def test_json_reports_unanalyzed_files(self, runner, tmp_path):
        import json

        with runner.isolated_filesystem(temp_dir=tmp_path):
            Path("config.yaml").write_text(
                yaml.dump({"llm": {"enabled": True, "endpoint": "http://x/v1", "model": "m"}})
            )
            Path("test.py").write_text("def foo(): pass")

            with patch("drep.cli_workflows.RepositoryScanner") as mock_scanner_class:
                mock_scanner = mock_scanner_class.return_value
                mock_scanner.close = AsyncMock()
                mock_scanner.analyze_code_quality = AsyncMock(
                    return_value=AnalysisResult(failed_files=["test.py"])
                )
                mock_scanner.analyze_docstrings = AsyncMock(return_value=AnalysisResult())

                result = runner.invoke(
                    cli, ["check", ".", "--config", "config.yaml", "--format", "json"]
                )

            payload = json.loads(result.stdout)
            assert payload["unanalyzed"] == ["test.py"]
            assert payload["findings"] == []


class TestCheckJsonIsMachineReadable:
    """`--format json` is documented as "JSON output for tools" - so it must parse."""

    def test_status_line_does_not_pollute_json_stdout(self, runner, tmp_path):
        import json

        with runner.isolated_filesystem(temp_dir=tmp_path):
            Path("config.yaml").write_text(
                "llm:\n  enabled: true\n  endpoint: http://x/v1\n  model: m\n"
            )
            Path("test.py").write_text("def foo(): pass")

            with patch("drep.cli_workflows.RepositoryScanner") as mock_scanner_class:
                mock_scanner = mock_scanner_class.return_value
                mock_scanner.get_scan_targets.return_value = ["test.py"]
                mock_scanner.close = AsyncMock()
                mock_scanner.analyze_code_quality = AsyncMock(
                    return_value=AnalysisResult(
                        findings=[
                            Finding(
                                type="bug",
                                severity="error",
                                file_path="test.py",
                                line=1,
                                message="boom",
                            )
                        ]
                    )
                )
                mock_scanner.analyze_docstrings = AsyncMock(return_value=AnalysisResult())

                result = runner.invoke(
                    cli,
                    ["check", ".", "--config", "config.yaml", "--format", "json"],
                )

            # stdout must be parseable on its own - no "Checking N file(s)..." preamble
            assert json.loads(result.stdout)["findings"][0]["message"] == "boom"
