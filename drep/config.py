"""Configuration loading and validation."""

import os
import re
from pathlib import Path

import yaml

from drep.models.config import Config


def load_config(config_path: str) -> Config:
    """Load and validate configuration from YAML file.

    Args:
        config_path: Path to the YAML configuration file

    Returns:
        Validated Config object

    Raises:
        FileNotFoundError: If config file doesn't exist
        yaml.YAMLError: If YAML is malformed
        pydantic.ValidationError: If config structure is invalid
    """
    config_file = Path(config_path)

    if not config_file.exists():
        raise FileNotFoundError(f"Config file not found: {config_path}")

    # Read YAML
    with config_file.open() as f:
        raw_config = yaml.safe_load(f)

    # Substitute environment variables
    config_str = yaml.dump(raw_config)
    config_str = _substitute_env_vars(config_str)
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
