"""`drep doctor` tests.

Reports what drep will actually do in this repository: which languages are
present, which deterministic tools will run, and which are configured but
missing. The installer shows this so a user sees their real coverage rather
than a promise.
"""

from pathlib import Path

from drep.cli import cli


class TestLanguageDetection:
    def test_reports_the_languages_present(self, runner, tmp_path):
        with runner.isolated_filesystem(temp_dir=tmp_path):
            Path("main.py").write_text("x = 1\n")
            Path("app.ts").write_text("const x = 1\n")
            Path("cmd").mkdir()
            Path("cmd/server.go").write_text("package main\n")

            result = runner.invoke(cli, ["doctor"])

            assert result.exit_code == 0
            assert "Python" in result.output
            assert "TypeScript" in result.output
            assert "Go" in result.output

    def test_does_not_report_languages_that_are_absent(self, runner, tmp_path):
        with runner.isolated_filesystem(temp_dir=tmp_path):
            Path("main.py").write_text("x = 1\n")

            result = runner.invoke(cli, ["doctor"])

            assert "Python" in result.output
            assert "Rust" not in result.output

    def test_ignores_vendored_trees(self, runner, tmp_path):
        with runner.isolated_filesystem(temp_dir=tmp_path):
            Path("main.py").write_text("x = 1\n")
            Path("node_modules/pkg").mkdir(parents=True)
            Path("node_modules/pkg/index.js").write_text("var x\n")

            result = runner.invoke(cli, ["doctor"])

            assert "JavaScript" not in result.output


class TestToolReporting:
    """The point of the command: what will actually run, and what will not."""

    def test_a_configured_and_present_tool_is_ready(self, runner, tmp_path):
        with runner.isolated_filesystem(temp_dir=tmp_path):
            Path("pyproject.toml").write_text("[tool.ruff]\n")
            Path("main.py").write_text("x = 1\n")

            result = runner.invoke(cli, ["doctor"])

            assert "ruff" in result.output
            assert "ready" in result.output.lower()

    def test_an_unconfigured_tool_says_so(self, runner, tmp_path):
        """Not an error: the project has not opted into that tool's style."""
        with runner.isolated_filesystem(temp_dir=tmp_path):
            Path("main.py").write_text("x = 1\n")

            result = runner.invoke(cli, ["doctor"])

            assert "not configured" in result.output.lower()
            assert result.exit_code == 0

    def test_a_configured_but_missing_tool_is_flagged(self, runner, tmp_path):
        with runner.isolated_filesystem(temp_dir=tmp_path):
            Path("tsconfig.json").write_text("{}\n")
            Path("app.ts").write_text("const x = 1\n")

            result = runner.invoke(cli, ["doctor"])

            assert "tsc" in result.output
            # Wording comes from runner.tool_status, the single derivation
            assert "configured but not found" in result.output.lower()
            # A gap in coverage the user should know about, but running doctor
            # is diagnosis, not a gate - it does not fail.
            assert result.exit_code == 0


class TestLLMReporting:
    def test_says_when_no_llm_is_configured(self, runner, tmp_path):
        with runner.isolated_filesystem(temp_dir=tmp_path):
            Path("main.py").write_text("x = 1\n")

            result = runner.invoke(cli, ["doctor"])

            assert "llm" in result.output.lower()
            # The deterministic half works without one, and that should be
            # obvious rather than looking like a broken install.
            assert "deterministic" in result.output.lower()
