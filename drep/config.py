"""Configuration loading and validation."""

import os
import re
from pathlib import Path

import click
import yaml

from drep.models.config import Config


def find_config_file(explicit_path: str | None = None) -> Path:
    """Find configuration file using standard search hierarchy.

    Search order (first found wins):
    1. Explicit path provided as argument
    2. DREP_CONFIG environment variable
    3. ./config.yaml (project-specific, current directory)
    4. ~/.config/drep/config.yaml (user config directory via XDG)

    Args:
        explicit_path: Explicit config path (highest priority)

    Returns:
        Path to config file (first one found, or user config path if none exist).
        The returned path may not exist yet - for `drep init`, it indicates where
        the config should be created. For other commands, use load_config() which
        validates file existence and raises FileNotFoundError if missing.
    """
    # 1. Explicit path (highest priority)
    if explicit_path:
        return Path(explicit_path)

    # 2. DREP_CONFIG environment variable
    env_path = os.environ.get("DREP_CONFIG")
    if env_path:
        return Path(env_path)

    # 3. Project-specific config (current directory)
    project_config = Path("config.yaml")
    if project_config.exists():
        return project_config

    # 4. User config directory (XDG standard via Click)
    user_config_dir = Path(click.get_app_dir("drep"))
    user_config = user_config_dir / "config.yaml"
    if user_config.exists():
        return user_config

    # If no config exists, return user config path as default
    # (for drep init and other commands to know where to create/look)
    return user_config


def get_user_config_dir() -> Path:
    """Get the user configuration directory for drep.

    Uses Click's get_app_dir() which follows platform conventions:
    - Linux: ~/.config/drep
    - macOS: ~/Library/Application Support/drep
    - Windows: C:\\Users\\<user>\\AppData\\Roaming\\drep

    Returns:
        Path to user config directory
    """
    return Path(click.get_app_dir("drep"))


def load_config(config_path: str, strict: bool = False, require_platform: bool = True) -> Config:
    """Load and validate configuration from YAML file.

    Args:
        config_path: Path to the YAML configuration file
        strict: If True, raise error if any ${VAR} placeholders remain after
                substitution (missing environment variables)
        require_platform: If True, require at least one platform (gitea/github/gitlab).
                         If False, allow LLM-only config (for pre-commit hooks).
                         Default: True (backward compatible)

    Returns:
        Validated Config object

    Raises:
        FileNotFoundError: If config file doesn't exist
        ValueError: If config is empty, malformed, or has missing env vars (strict mode)
        yaml.YAMLError: If YAML is malformed
        pydantic.ValidationError: If config structure is invalid

    Note:
        Setting require_platform=False is useful for pre-commit hooks where
        you want local-only analysis without platform API integration.
    """
    config_file = Path(config_path)

    if not config_file.exists():
        raise FileNotFoundError(f"Config file not found: {config_path}")

    # Read YAML
    with config_file.open() as f:
        raw_config = yaml.safe_load(f)

    # Check for empty or malformed YAML
    if raw_config is None:
        raise ValueError(f"Config file is empty: {config_path}")
    if not isinstance(raw_config, dict):
        raise ValueError(
            f"Config file must contain a YAML mapping/dictionary, "
            f"got {type(raw_config).__name__}: {config_path}"
        )

    # Substitute environment variables
    config_str = yaml.dump(raw_config)
    config_str = _substitute_env_vars(config_str)

    # In strict mode, check for remaining placeholders
    if strict:
        remaining = re.findall(r"\$\{([^}]+)\}", config_str)
        if remaining:
            raise ValueError(
                f"Missing required environment variables: {', '.join(sorted(set(remaining)))}"
            )

    config_dict = yaml.safe_load(config_str)

    # Pass require_platform flag to Config model
    config_dict["require_platform_config"] = require_platform

    # Validate with Pydantic
    return Config(**config_dict)


def _substitute_env_vars(text: str) -> str:
    """Replace ${VAR_NAME} with environment variable values.

    Args:
        text: Text containing ${VAR_NAME} patterns

    Returns:
        Text with variables substituted (or left as-is if not set)
    """
    pattern = r"\$\{([^}]+)\}"

    def replacer(match):
        var_name = match.group(1)
        return os.environ.get(var_name, match.group(0))

    return re.sub(pattern, replacer, text)
