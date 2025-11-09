"""Tests for configuration file discovery."""

from pathlib import Path
from unittest.mock import patch

from drep.config import find_config_file, get_user_config_dir


class TestGetUserConfigDir:
    """Tests for get_user_config_dir function."""

    def test_returns_path_object(self):
        """Test that get_user_config_dir returns a Path object."""
        result = get_user_config_dir()
        assert isinstance(result, Path)

    def test_contains_drep_directory(self):
        """Test that path contains 'drep' directory name."""
        result = get_user_config_dir()
        assert "drep" in str(result).lower()


class TestFindConfigFile:
    """Tests for find_config_file function."""

    def test_explicit_path_takes_precedence(self, tmp_path):
        """Test explicit path has highest priority."""
        explicit = tmp_path / "explicit.yaml"
        explicit.write_text("test")

        result = find_config_file(str(explicit))
        assert result == explicit

    def test_env_var_takes_precedence_over_discovery(self, tmp_path, monkeypatch):
        """Test DREP_CONFIG environment variable takes precedence."""
        env_config = tmp_path / "env_config.yaml"
        env_config.write_text("test")

        monkeypatch.setenv("DREP_CONFIG", str(env_config))

        result = find_config_file(None)
        assert result == env_config

    def test_project_config_found_in_current_directory(self, tmp_path, monkeypatch):
        """Test finds config.yaml in current directory."""
        project_config = tmp_path / "config.yaml"
        project_config.write_text("test")

        # Change to tmp directory
        monkeypatch.chdir(tmp_path)

        result = find_config_file(None)
        assert result == Path("config.yaml")
        assert result.exists()

    def test_user_config_found_when_project_not_exists(self, tmp_path, monkeypatch):
        """Test falls back to user config when no project config exists."""
        # Change to tmp directory with no config.yaml
        monkeypatch.chdir(tmp_path)

        with patch("drep.config.click.get_app_dir") as mock_get_app_dir:
            user_dir = tmp_path / "user_config"
            user_dir.mkdir()
            user_config = user_dir / "config.yaml"
            user_config.write_text("test")

            mock_get_app_dir.return_value = str(user_dir)

            result = find_config_file(None)
            assert result == user_config

    def test_returns_user_config_path_when_no_config_exists(self, tmp_path, monkeypatch):
        """Test returns user config path when no config exists anywhere."""
        # Change to tmp directory with no config.yaml
        monkeypatch.chdir(tmp_path)

        with patch("drep.config.click.get_app_dir") as mock_get_app_dir:
            user_dir = tmp_path / "user_config"
            user_dir.mkdir()
            # Don't create the config file

            mock_get_app_dir.return_value = str(user_dir)

            result = find_config_file(None)
            assert result == user_dir / "config.yaml"
            assert not result.exists()  # File doesn't exist, just the path

    def test_search_order_explicit_over_env(self, tmp_path, monkeypatch):
        """Test explicit path overrides environment variable."""
        explicit = tmp_path / "explicit.yaml"
        env_config = tmp_path / "env.yaml"
        explicit.write_text("explicit")
        env_config.write_text("env")

        monkeypatch.setenv("DREP_CONFIG", str(env_config))

        result = find_config_file(str(explicit))
        assert result == explicit
        assert result != env_config

    def test_search_order_env_over_project(self, tmp_path, monkeypatch):
        """Test environment variable overrides project config."""
        project_config = tmp_path / "config.yaml"
        env_config = tmp_path / "env.yaml"
        project_config.write_text("project")
        env_config.write_text("env")

        monkeypatch.setenv("DREP_CONFIG", str(env_config))
        monkeypatch.chdir(tmp_path)

        result = find_config_file(None)
        assert result == env_config
        assert result != project_config

    def test_search_order_project_over_user(self, tmp_path, monkeypatch):
        """Test project config overrides user config."""
        project_config = tmp_path / "config.yaml"
        project_config.write_text("project")

        monkeypatch.chdir(tmp_path)

        with patch("drep.config.click.get_app_dir") as mock_get_app_dir:
            user_dir = tmp_path / "user"
            user_dir.mkdir()
            user_config = user_dir / "config.yaml"
            user_config.write_text("user")

            mock_get_app_dir.return_value = str(user_dir)

            result = find_config_file(None)
            assert result == Path("config.yaml")  # Project config is relative
            assert result.exists()
            assert result.read_text() == "project"

    def test_handles_empty_string_as_none(self):
        """Test empty string is treated as None."""
        result_none = find_config_file(None)
        result_empty = find_config_file("")

        # Both should return same path (user config dir)
        assert result_none == result_empty

    def test_handles_relative_paths(self, tmp_path, monkeypatch):
        """Test handles relative paths correctly."""
        config = tmp_path / "subdir" / "config.yaml"
        config.parent.mkdir()
        config.write_text("test")

        monkeypatch.chdir(tmp_path)

        result = find_config_file("subdir/config.yaml")
        assert result == Path("subdir/config.yaml")
