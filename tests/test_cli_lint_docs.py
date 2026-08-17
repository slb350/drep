"""lint-docs command tests.

lint-docs is the rule-based markdown gate: no LLM, no config, no platform. It
is the half of `drep` that can sit in front of every commit, so discovery and
exit codes have to be exactly right.
"""

from pathlib import Path

from drep.cli import cli

# A bare URL reliably trips the bare_url pattern check.
DIRTY_MARKDOWN = "# Title\n\nSee https://example.com for details.\n"
CLEAN_MARKDOWN = "# Title\n\nSee [the docs](https://example.com) for details.\n"


class TestLintDocsDiscovery:
    """Which files lint-docs picks up when handed a directory."""

    def test_skips_ignored_directories(self, runner, tmp_path):
        """Vendored trees are not the user's documentation.

        Walking venv/ buries real findings under third-party licence files, and
        file_targets.is_ignored_dir is the project's single answer to which
        directories to skip.
        """
        with runner.isolated_filesystem(temp_dir=tmp_path):
            Path("README.md").write_text(CLEAN_MARKDOWN)
            for ignored in ("venv/lib", "__pycache__", "drep.egg-info"):
                Path(ignored).mkdir(parents=True)
                (Path(ignored) / "LICENSE.md").write_text(DIRTY_MARKDOWN)

            result = runner.invoke(cli, ["lint-docs", "."])

            assert result.exit_code == 0
            assert "LICENSE.md" not in result.output
            assert "1 markdown files" in result.output

    def test_accepts_multiple_paths(self, runner, tmp_path):
        """pre-commit passes each staged file, so lint-docs must take a list."""
        with runner.isolated_filesystem(temp_dir=tmp_path):
            Path("one.md").write_text(DIRTY_MARKDOWN)
            Path("two.md").write_text(DIRTY_MARKDOWN)
            Path("three.md").write_text(DIRTY_MARKDOWN)

            result = runner.invoke(cli, ["lint-docs", "one.md", "two.md"])

            assert result.exit_code == 0
            assert "one.md" in result.output
            assert "two.md" in result.output
            assert "three.md" not in result.output

    def test_defaults_to_current_directory(self, runner, tmp_path):
        """No argument keeps the documented `drep lint-docs` shorthand working."""
        with runner.isolated_filesystem(temp_dir=tmp_path):
            Path("README.md").write_text(DIRTY_MARKDOWN)

            result = runner.invoke(cli, ["lint-docs"])

            assert result.exit_code == 0
            assert "README.md" in result.output


class TestLintDocsStrict:
    """--strict turns the report into a gate."""

    def test_strict_exits_nonzero_on_issues(self, runner, tmp_path):
        with runner.isolated_filesystem(temp_dir=tmp_path):
            Path("README.md").write_text(DIRTY_MARKDOWN)

            result = runner.invoke(cli, ["lint-docs", "--strict", "README.md"])

            assert result.exit_code == 1

    def test_strict_exits_zero_when_clean(self, runner, tmp_path):
        with runner.isolated_filesystem(temp_dir=tmp_path):
            Path("README.md").write_text(CLEAN_MARKDOWN)

            result = runner.invoke(cli, ["lint-docs", "--strict", "README.md"])

            assert result.exit_code == 0

    def test_reporting_mode_stays_zero(self, runner, tmp_path):
        """Without --strict lint-docs keeps its existing report-only contract."""
        with runner.isolated_filesystem(temp_dir=tmp_path):
            Path("README.md").write_text(DIRTY_MARKDOWN)

            result = runner.invoke(cli, ["lint-docs", "README.md"])

            assert result.exit_code == 0
