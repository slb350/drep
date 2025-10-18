"""Configuration loading and validation."""

import os
import re
from pathlib import Path

import yaml

from drep.models.config import Config


def load_config(config_path: str, strict: bool = False) -> Config:
    """Load and validate configuration from YAML file.

    Args:
        config_path: Path to the YAML configuration file
        strict: If True, raise error if any ${VAR} placeholders remain after
                substitution (missing environment variables)

    Returns:
        Validated Config object

    Raises:
        FileNotFoundError: If config file doesn't exist
        ValueError: If config is empty, malformed, or has missing env vars (strict mode)
        yaml.YAMLError: If YAML is malformed
        pydantic.ValidationError: If config structure is invalid
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
