"""`drep init-llm` tests.

Writes just the llm block of config.yaml from a named preset. Exists so the
installer can set up a model without the full `drep init` wizard, which also
demands platform credentials a local gate never uses - and so the preset table
lives in Python rather than being duplicated in shell.
"""

from pathlib import Path

import yaml

from drep.cli import cli


class TestPresetSelection:
    def test_writes_a_config_from_a_preset(self, runner, tmp_path):
        with runner.isolated_filesystem(temp_dir=tmp_path):
            result = runner.invoke(cli, ["init-llm", "--provider", "openrouter"])

            assert result.exit_code == 0
            config = yaml.safe_load(Path("config.yaml").read_text())
            assert config["llm"]["endpoint"] == "https://openrouter.ai/api/v1"
            assert config["llm"]["api_key"] == "${OPENROUTER_API_KEY}"

    def test_model_can_be_overridden(self, runner, tmp_path):
        with runner.isolated_filesystem(temp_dir=tmp_path):
            runner.invoke(cli, ["init-llm", "--provider", "local", "--model", "my-model"])

            config = yaml.safe_load(Path("config.yaml").read_text())
            assert config["llm"]["model"] == "my-model"

    def test_local_writes_no_api_key(self, runner, tmp_path):
        with runner.isolated_filesystem(temp_dir=tmp_path):
            runner.invoke(cli, ["init-llm", "--provider", "local"])

            config = yaml.safe_load(Path("config.yaml").read_text())
            assert "api_key" not in config["llm"]

    def test_custom_requires_an_endpoint(self, runner, tmp_path):
        with runner.isolated_filesystem(temp_dir=tmp_path):
            result = runner.invoke(cli, ["init-llm", "--provider", "custom"])

            assert result.exit_code == 1
            assert "endpoint" in result.output.lower()

    def test_unknown_provider_is_rejected(self, runner, tmp_path):
        with runner.isolated_filesystem(temp_dir=tmp_path):
            result = runner.invoke(cli, ["init-llm", "--provider", "nope"])

            assert result.exit_code != 0


class TestSecretHandling:
    """The key never lands in the file; only the variable name does."""

    def test_the_secret_itself_is_never_written(self, runner, tmp_path):
        with runner.isolated_filesystem(temp_dir=tmp_path):
            runner.invoke(cli, ["init-llm", "--provider", "openrouter"])

            written = Path("config.yaml").read_text()
            assert "${OPENROUTER_API_KEY}" in written
            assert "sk-" not in written

    def test_reminds_the_user_to_export_the_key(self, runner, tmp_path):
        with runner.isolated_filesystem(temp_dir=tmp_path):
            result = runner.invoke(cli, ["init-llm", "--provider", "openrouter"])

            assert "OPENROUTER_API_KEY" in result.output
            assert "export" in result.output


class TestExistingConfig:
    def test_preserves_other_sections(self, runner, tmp_path):
        with runner.isolated_filesystem(temp_dir=tmp_path):
            Path("config.yaml").write_text(
                yaml.dump({"github": {"token": "${GITHUB_TOKEN}", "repositories": ["o/r"]}})
            )

            runner.invoke(cli, ["init-llm", "--provider", "local"])

            config = yaml.safe_load(Path("config.yaml").read_text())
            assert config["github"]["repositories"] == ["o/r"]
            assert config["llm"]["provider"] == "openai-compatible"

    def test_refuses_to_clobber_without_force(self, runner, tmp_path):
        with runner.isolated_filesystem(temp_dir=tmp_path):
            Path("config.yaml").write_text(
                yaml.dump({"llm": {"enabled": True, "endpoint": "http://keep/v1", "model": "keep"}})
            )

            result = runner.invoke(cli, ["init-llm", "--provider", "local"])

            assert result.exit_code == 1
            config = yaml.safe_load(Path("config.yaml").read_text())
            assert config["llm"]["model"] == "keep"

    def test_force_overwrites(self, runner, tmp_path):
        with runner.isolated_filesystem(temp_dir=tmp_path):
            Path("config.yaml").write_text(
                yaml.dump({"llm": {"enabled": True, "endpoint": "http://old/v1", "model": "old"}})
            )

            result = runner.invoke(cli, ["init-llm", "--provider", "local", "--force"])

            assert result.exit_code == 0
            config = yaml.safe_load(Path("config.yaml").read_text())
            assert config["llm"]["model"] != "old"
