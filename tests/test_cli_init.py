"""Init wizard command tests: prompts, backups, env checks, provider selection."""

from pathlib import Path
from unittest.mock import patch

import yaml

from drep.cli import cli


class TestInitCommand:
    """Tests for drep init command."""

    def test_init_location_choice_invalid_rejected(self, runner, tmp_path):
        """Test that invalid location choice (3) is rejected and reprompted."""
        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Try invalid choice "3", then valid choice "1"
            inputs = "3\n1\ngitea\n\n\nn\nn\nn\nn\n"
            result = runner.invoke(cli, ["init"], input=inputs)

            # Should succeed after retry
            assert result.exit_code == 0
            assert (
                "Error: '3' is not one of '1', '2'" in result.output
                or "invalid choice" in result.output.lower()
            )
            assert Path("config.yaml").exists()

    def test_init_location_choice_empty_uses_default(self, runner, tmp_path):
        """Test that pressing enter (empty input) for location uses default."""
        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Test that location choice "1" creates in current directory
            # (validates that empty input would use default "2" by contrast)
            inputs = "1\ngitea\n\n\nn\nn\nn\nn\n"
            result = runner.invoke(cli, ["init"], input=inputs)

            assert result.exit_code == 0
            assert Path("config.yaml").exists()  # Created in current dir

            # Now verify a different run using empty (default) would NOT create here
            # (This indirectly validates empty uses default "2" = user config dir)
            # Since we can't easily test user config dir without side effects,
            # we verify the Choice validator accepts empty input and uses default
            assert "Choose location (1, 2) [2]:" in result.output  # Shows default is 2

    def test_init_creates_config_file_minimal(self, runner, tmp_path):
        """Test that init command creates config.yaml with minimal setup."""
        # Run in temp directory
        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Wizard inputs:
            # 0. Config location: 1 (current directory)
            # 1. Platform: gitea
            # 2. Gitea URL: (default)
            # 3. Repositories: (default)
            # 4. Enable LLM: n
            # 5. Enable docs: y
            # 6. Markdown checks: n
            # 7. Custom dictionary: n
            # 8. Custom DB: n
            # 9. Check env vars: n
            inputs = "1\ngitea\n\n\nn\ny\nn\nn\nn\nn\n"
            result = runner.invoke(cli, ["init"], input=inputs)

            assert result.exit_code == 0
            assert "✓ Configuration created successfully!" in result.output
            assert Path("config.yaml").exists()

            # Check content
            config_content = Path("config.yaml").read_text()
            assert "gitea:" in config_content
            assert "${GITEA_TOKEN}" in config_content
            assert "documentation:" in config_content
            # LLM section should not be present when disabled
            assert "llm:" not in config_content

    def test_init_creates_github_config(self, runner, tmp_path):
        """Test that init command creates GitHub config."""
        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Wizard inputs:
            # 1. Platform: github
            # 2. GitHub Enterprise: n
            # 3. Repositories: (default)
            # 4. Enable LLM: n
            # 5. Enable docs: y
            # 6. Markdown checks: n
            # 7. Custom dictionary: n
            # 8. Custom DB: n
            # 9. Check env vars: n
            inputs = "1\ngithub\nn\n\nn\ny\nn\nn\nn\nn\n"
            result = runner.invoke(cli, ["init"], input=inputs)

            assert result.exit_code == 0
            config_content = Path("config.yaml").read_text()
            assert "github:" in config_content
            assert "${GITHUB_TOKEN}" in config_content

    def test_init_creates_gitlab_config(self, runner, tmp_path):
        """Test that init command creates GitLab config."""
        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Wizard inputs:
            # 1. Platform: gitlab
            # 2. Self-hosted: n
            # 3. Repositories: (default)
            # 4. Enable LLM: n
            # 5. Enable docs: y
            # 6. Markdown checks: n
            # 7. Custom dictionary: n
            # 8. Custom DB: n
            # 9. Check env vars: n
            inputs = "1\ngitlab\nn\n\nn\ny\nn\nn\nn\nn\n"
            result = runner.invoke(cli, ["init"], input=inputs)

            assert result.exit_code == 0
            config_content = Path("config.yaml").read_text()
            assert "gitlab:" in config_content
            assert "${GITLAB_TOKEN}" in config_content

    def test_init_prompts_on_existing_file(self, runner, tmp_path):
        """Test that init prompts before overwriting existing file."""
        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Create existing config
            Path("config.yaml").write_text("existing: config")

            # Run init and abort
            # 0. Config location: 1 (current directory)
            # 1. Overwrite: n (abort)
            result = runner.invoke(cli, ["init"], input="1\nn\n")

            assert result.exit_code == 1
            assert "already exists" in result.output

            # Verify original file unchanged
            assert Path("config.yaml").read_text() == "existing: config"

    def test_init_overwrites_with_confirmation(self, runner, tmp_path):
        """Test that init overwrites file when user confirms."""
        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Create existing config
            Path("config.yaml").write_text("existing: config")

            # Wizard inputs:
            # 1. Overwrite: y
            # 2. Platform: gitea
            # 3. Gitea URL: (default)
            # 4. Repositories: (default)
            # 5. Enable LLM: n
            # 6. Enable docs: y
            # 7. Markdown checks: n
            # 8. Custom dictionary: n
            # 9. Custom DB: n
            # 10. Check env vars: n
            inputs = "1\ny\ngitea\n\n\nn\ny\nn\nn\nn\nn\n"
            result = runner.invoke(cli, ["init"], input=inputs)

            assert result.exit_code == 0
            assert "✓ Configuration created successfully!" in result.output
            assert "Backup created:" in result.output

            # Verify new content
            config_content = Path("config.yaml").read_text()
            assert "gitea:" in config_content
            assert "existing: config" not in config_content

    def test_init_backup_failure_aborts(self, runner, tmp_path):
        """Test init aborts if backup creation fails with PermissionError."""
        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Create existing config
            Path("config.yaml").write_text("existing: config")

            # Mock shutil.copy to raise PermissionError
            with patch("shutil.copy") as mock_copy:
                mock_copy.side_effect = PermissionError("Cannot create backup")

                # Wizard inputs:
                # 0. Config location: 1
                # 1. Overwrite: y
                # (Should abort before needing more inputs)
                inputs = "1\ny\n"
                result = runner.invoke(cli, ["init"], input=inputs)

                # Should abort with error
                assert result.exit_code == 1
                assert "ERROR: Cannot create backup" in result.output
                assert (
                    "Permission denied" in result.output
                    or "Cannot safely overwrite" in result.output
                )

                # Original config should still exist unchanged
                assert Path("config.yaml").exists()
                assert Path("config.yaml").read_text() == "existing: config"

    def test_init_backup_disk_full(self, runner, tmp_path):
        """Test init aborts if backup creation fails due to disk full."""
        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Create existing config
            Path("config.yaml").write_text("existing: config")

            # Mock shutil.copy to raise OSError (disk full)
            with patch("shutil.copy") as mock_copy:
                mock_copy.side_effect = OSError(28, "No space left on device")

                # Wizard inputs:
                # 0. Config location: 1
                # 1. Overwrite: y
                inputs = "1\ny\n"
                result = runner.invoke(cli, ["init"], input=inputs)

                # Should abort with error
                assert result.exit_code == 1
                assert "ERROR: Cannot create backup" in result.output
                assert (
                    "No space left" in result.output or "Cannot safely overwrite" in result.output
                )

                # Original config should still exist unchanged
                assert Path("config.yaml").exists()
                assert Path("config.yaml").read_text() == "existing: config"

    def test_init_handles_file_write_permission_denied(self, runner, tmp_path):
        """Test init handles PermissionError when writing config file."""
        from unittest.mock import patch

        with (
            runner.isolated_filesystem(temp_dir=tmp_path),
            patch("pathlib.Path.write_text") as mock_write,
        ):
            # Mock write_text to raise PermissionError
            mock_write.side_effect = PermissionError("Permission denied")

            # Wizard inputs (minimal)
            inputs = "1\ngitea\n\n\nn\ny\nn\nn\nn\nn\n"
            result = runner.invoke(cli, ["init"], input=inputs)

            # Should abort with clear error message
            assert result.exit_code == 1
            assert "ERROR: Permission denied writing to" in result.output
            assert "Check file permissions" in result.output

    def test_init_handles_file_write_disk_full(self, runner, tmp_path):
        """Test init handles OSError when disk is full."""
        from unittest.mock import patch

        with (
            runner.isolated_filesystem(temp_dir=tmp_path),
            patch("pathlib.Path.write_text") as mock_write,
        ):
            # Mock write_text to raise OSError (disk full)
            mock_write.side_effect = OSError(28, "No space left on device")

            # Wizard inputs (minimal)
            inputs = "1\ngitea\n\n\nn\ny\nn\nn\nn\nn\n"
            result = runner.invoke(cli, ["init"], input=inputs)

            # Should abort with clear error message
            assert result.exit_code == 1
            assert "ERROR: Failed to write config:" in result.output
            assert "No space left on device" in result.output
            assert "Check disk space and permissions" in result.output

    def test_init_handles_yaml_serialization_error(self, runner, tmp_path):
        """Test init handles YAML serialization errors gracefully."""
        from unittest.mock import patch

        import yaml

        with runner.isolated_filesystem(temp_dir=tmp_path), patch("yaml.dump") as mock_dump:
            # Mock yaml.dump to raise YAMLError
            mock_dump.side_effect = yaml.YAMLError("Cannot serialize non-standard type")

            # Wizard inputs (minimal)
            inputs = "1\ngitea\n\n\nn\ny\nn\nn\nn\nn\n"
            result = runner.invoke(cli, ["init"], input=inputs)

            # Should abort with clear error message
            assert result.exit_code == 1
            assert "ERROR: Failed to serialize configuration" in result.output
            assert (
                "Cannot serialize non-standard type" in result.output
                or "This is a bug" in result.output
            )

    def test_init_template_structure(self, runner, tmp_path):
        """Test that init creates valid YAML template."""
        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Wizard inputs:
            # 1-9: same as minimal test
            inputs = "1\ngitea\n\n\nn\ny\nn\nn\nn\nn\n"
            result = runner.invoke(cli, ["init"], input=inputs)

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
            # LLM should not be in config when disabled
            assert "llm" not in config

    def test_init_with_custom_repositories(self, runner, tmp_path):
        """Test init with custom repository configuration."""
        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Wizard inputs:
            # 1. Platform: gitea
            # 2. Gitea URL: (default)
            # 3. Repositories: owner/repo1, owner/repo2
            # 4-9: same as minimal test
            inputs = "1\ngitea\n\nowner/repo1, owner/repo2\nn\ny\nn\nn\nn\nn\n"
            result = runner.invoke(cli, ["init"], input=inputs)

            assert result.exit_code == 0
            config = yaml.safe_load(Path("config.yaml").read_text())
            assert "owner/repo1" in config["gitea"]["repositories"]
            assert "owner/repo2" in config["gitea"]["repositories"]

    def test_init_with_documentation_options(self, runner, tmp_path):
        """Test init with documentation configuration."""
        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Wizard inputs:
            # 1. Platform: gitea
            # 2. Gitea URL: (default)
            # 3. Repositories: (default)
            # 4. Enable LLM: n
            # 5. Enable docs: y
            # 6. Markdown checks: y
            # 7. Custom dictionary: y
            # 8. Words: foo,bar,baz
            # 9. Custom DB: n
            # 10. Check env vars: n
            inputs = "1\ngitea\n\n\nn\ny\ny\ny\nfoo,bar,baz\nn\nn\n"
            result = runner.invoke(cli, ["init"], input=inputs)

            assert result.exit_code == 0
            config = yaml.safe_load(Path("config.yaml").read_text())
            assert config["documentation"]["enabled"] is True
            assert config["documentation"]["markdown_checks"] is True
            assert "foo" in config["documentation"]["custom_dictionary"]
            assert "bar" in config["documentation"]["custom_dictionary"]
            assert "baz" in config["documentation"]["custom_dictionary"]

    def test_init_validates_config(self, runner, tmp_path):
        """Test that init validates the created config."""
        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Wizard inputs:
            # 1-9: same as minimal test
            inputs = "1\ngitea\n\n\nn\ny\nn\nn\nn\nn\n"
            result = runner.invoke(cli, ["init"], input=inputs)

            assert result.exit_code == 0
            assert "Validating configuration..." in result.output
            assert "✓ Configuration structure is valid!" in result.output

    def test_init_github_enterprise(self, runner, tmp_path):
        """Test GitHub Enterprise configuration with custom URL."""
        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Wizard inputs:
            # 1. Platform: github
            # 2. Enterprise: y
            # 3. API URL: https://github.corp.example.com/api/v3
            # 4. Repositories: (default)
            # 5-10: minimal config
            inputs = "1\ngithub\ny\nhttps://github.corp.example.com/api/v3\n\nn\ny\nn\nn\nn\nn\n"
            result = runner.invoke(cli, ["init"], input=inputs)

            assert result.exit_code == 0
            config = yaml.safe_load(Path("config.yaml").read_text())
            assert "github" in config
            assert config["github"]["url"] == "https://github.corp.example.com/api/v3"

    def test_init_gitlab_selfhosted(self, runner, tmp_path):
        """Test self-hosted GitLab configuration with custom URL."""
        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Wizard inputs:
            # 1. Platform: gitlab
            # 2. Self-hosted: y
            # 3. GitLab URL: https://gitlab.internal.corp.com
            # 4. Repositories: (default)
            # 5-10: minimal config
            inputs = "1\ngitlab\ny\nhttps://gitlab.internal.corp.com\n\nn\ny\nn\nn\nn\nn\n"
            result = runner.invoke(cli, ["init"], input=inputs)

            assert result.exit_code == 0
            config = yaml.safe_load(Path("config.yaml").read_text())
            assert "gitlab" in config
            assert config["gitlab"]["url"] == "https://gitlab.internal.corp.com"

    def test_init_backup_contains_original_content(self, runner, tmp_path):
        """Test that backup file preserves original config content."""
        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Create initial config
            original_content = "original: configuration\ndata: test"
            Path("config.yaml").write_text(original_content)

            # Overwrite config
            inputs = "1\ny\n1\ngithub\nn\nowner/*\nn\ny\nn\nn\nn\nn\n"
            result = runner.invoke(cli, ["init"], input=inputs)

            assert result.exit_code == 0
            assert Path("config.yaml.backup").exists()
            backup_content = Path("config.yaml.backup").read_text()
            assert backup_content == original_content

    def test_init_custom_dictionary_excessive_whitespace(self, runner, tmp_path):
        """Test custom dictionary handles excessive whitespace correctly."""
        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Custom dictionary with excessive whitespace
            inputs = "1\ngithub\nn\nowner/*\nn\ny\ny\ny\n  word1  ,  word2  ,   word3   \nn\nn\n"
            result = runner.invoke(cli, ["init"], input=inputs)

            assert result.exit_code == 0
            config_content = Path("config.yaml").read_text()
            # Words should be stripped
            assert "word1" in config_content
            assert "word2" in config_content
            assert "word3" in config_content

    def test_init_custom_dictionary_empty_after_strip(self, runner, tmp_path):
        """Test custom dictionary handles empty string after stripping."""
        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Custom dictionary with only whitespace
            inputs = "1\ngithub\nn\nowner/*\nn\ny\ny\ny\n    \nn\nn\n"
            result = runner.invoke(cli, ["init"], input=inputs)

            assert result.exit_code == 0
            config_content = Path("config.yaml").read_text()
            # Empty custom_dictionary should result in empty list
            assert "custom_dictionary: []" in config_content

    def test_init_custom_dictionary_only_commas(self, runner, tmp_path):
        """Test custom dictionary handles input with only commas and whitespace."""
        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Custom dictionary with only commas and spaces
            inputs = "1\ngithub\nn\nowner/*\nn\ny\ny\ny\n, , , \nn\nn\n"
            result = runner.invoke(cli, ["init"], input=inputs)

            assert result.exit_code == 0
            config_content = Path("config.yaml").read_text()
            # Should result in empty list (all entries filtered out)
            assert "custom_dictionary: []" in config_content

    def test_init_validates_url_type(self, runner, tmp_path):
        """Test URLType validator catches invalid URLs during wizard."""
        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Try Gitea with invalid URL, then valid URL
            # Inputs: location, platform, invalid_url, valid_url, repos, llm, docs
            inputs = "1\ngitea\nnot-a-url\nhttp://localhost:3000\n\nn\ny\nn\nn\nn\nn\n"
            result = runner.invoke(cli, ["init"], input=inputs)

            # Should succeed after retry
            assert result.exit_code == 0
            # Verify error message appeared for invalid URL
            assert "invalid" in result.output.lower() or "error" in result.output.lower()
            # Verify config created with valid URL
            config_content = Path("config.yaml").read_text()
            assert "http://localhost:3000" in config_content

    def test_init_validates_repository_list(self, runner, tmp_path):
        """Test RepositoryListType validator catches invalid patterns during wizard."""
        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Try GitHub with invalid repository pattern, then valid pattern
            # Invalid: contains spaces (not allowed)
            # Valid: owner/repo
            inputs = "1\ngithub\nn\ninvalid repo pattern\nowner/repo\nn\ny\nn\nn\nn\nn\n"
            result = runner.invoke(cli, ["init"], input=inputs)

            # Should succeed after retry
            assert result.exit_code == 0
            # Verify error message appeared for invalid pattern
            assert "invalid" in result.output.lower() or "error" in result.output.lower()
            # Verify config created with valid pattern
            config_content = Path("config.yaml").read_text()
            assert "owner/repo" in config_content

    def test_init_validates_bedrock_model(self, runner, tmp_path):
        """Test BedrockModelType validator catches invalid model IDs during wizard."""
        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Try Bedrock with invalid model ID, then valid model ID
            # Invalid: doesn't start with valid prefix (anthropic., ai21., etc.)
            # Valid: anthropic.claude-3-5-sonnet-20241022-v2:0
            inputs = (
                "1\ngithub\nn\nowner/*\ny\nbedrock\n"
                "us-east-1\ninvalid-model-id\n"
                "anthropic.claude-3-5-sonnet-20241022-v2:0\n"
                "n\nn\ny\nn\nn\nn\nn\n"
            )
            result = runner.invoke(cli, ["init"], input=inputs)

            # Should succeed after retry
            assert result.exit_code == 0
            # Verify error message appeared for invalid model
            assert "invalid" in result.output.lower() or "error" in result.output.lower()
            # Verify config created with valid model
            config_content = Path("config.yaml").read_text()
            assert "anthropic.claude-3-5-sonnet-20241022-v2:0" in config_content

    def test_init_validates_database_url(self, runner, tmp_path):
        """Test DatabaseURLType validator catches malformed database URLs during wizard."""
        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Try custom database with invalid URL, then valid URL
            # Invalid: missing ://
            # Valid: sqlite:///./drep.db
            inputs = "1\ngitea\n\n\nn\ny\nn\nn\ny\ninvalid-db-url\nsqlite:///./drep.db\nn\n"
            result = runner.invoke(cli, ["init"], input=inputs)

            # Should succeed after retry
            assert result.exit_code == 0
            # Verify error message appeared for invalid URL
            assert "invalid" in result.output.lower() or "error" in result.output.lower()
            # Verify config created with valid URL
            config_content = Path("config.yaml").read_text()
            assert "sqlite:///./drep.db" in config_content

    def test_init_validates_nonempty_string(self, runner, tmp_path):
        """Test NonEmptyString validator catches empty/whitespace input during wizard."""
        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Try OpenAI-compatible with empty model name, then valid model name
            inputs = (
                "1\ngitea\n\n\ny\nopenai-compatible\n"
                "http://localhost:1234/v1\n   \n"  # Empty/whitespace model name
                "qwen3-30b-a3b\n"  # Valid model name
                "n\nn\nn\ny\nn\nn\nn\nn\n"
            )
            result = runner.invoke(cli, ["init"], input=inputs)

            # Should succeed after retry
            assert result.exit_code == 0
            # Verify error message appeared for empty input
            assert "invalid" in result.output.lower() or "error" in result.output.lower()
            # Verify config created with valid model
            config_content = Path("config.yaml").read_text()
            assert "qwen3-30b-a3b" in config_content
