"""Configuration loading and validation."""

import os
import re
from pathlib import Path
from typing import Any

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

    # Substitute environment variables over the parsed tree.
    # Values are substituted as plain strings — they must never be re-parsed
    # as YAML (the old dump->substitute->reload round-trip re-typed "true"
    # to bool, "123" to int, and let values like "a: b" corrupt the structure).
    unresolved: set[str] = set()
    raw_config = _substitute_tree(raw_config, unresolved)

    # In strict mode, fail on any placeholders that survived substitution
    if strict and unresolved:
        raise ValueError(f"Missing required environment variables: {', '.join(sorted(unresolved))}")

    # Pass require_platform flag to Config model
    raw_config["require_platform_config"] = require_platform

    # Validate with Pydantic
    return Config(**raw_config)


_PLACEHOLDER_RE = re.compile(r"\$\{([^}]+)\}")


def _substitute_tree(node: Any, unresolved: set[str]) -> Any:
    """Recursively substitute ${VAR_NAME} placeholders in string values of a parsed tree.

    Dicts and lists are walked; only strings are examined. Substituted values
    are inserted as plain strings (never re-parsed as YAML). Placeholders whose
    variables are unset are left in place and their names collected in
    ``unresolved``.
    """
    if isinstance(node, dict):
        return {key: _substitute_tree(value, unresolved) for key, value in node.items()}
    if isinstance(node, list):
        return [_substitute_tree(item, unresolved) for item in node]
    if isinstance(node, str):

        def replacer(match: re.Match[str]) -> str:
            var_name = match.group(1)
            value = os.environ.get(var_name)
            if value is None:
                unresolved.add(var_name)
                return match.group(0)
            return value

        return _PLACEHOLDER_RE.sub(replacer, node)
    return node
