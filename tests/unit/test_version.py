"""Tests for version single-sourcing across the package."""

from pathlib import Path

import tomllib

from drep import __version__


def test_pyproject_version_matches_package():
    """pyproject.toml version is derived from drep.__version__ — they must agree."""
    pyproject = Path(__file__).parents[2] / "pyproject.toml"
    with pyproject.open("rb") as f:
        data = tomllib.load(f)

    if "version" in data["project"]:
        # Static version declared — must equal the package version.
        assert data["project"]["version"] == __version__
    else:
        # Dynamic version — must be sourced from the package attribute.
        version_config = data["tool"]["hatch"]["version"]
        assert version_config["path"] == "drep/__init__.py"


def test_server_app_uses_package_version():
    """FastAPI app version is the package version, not a hardcoded string."""
    from drep.server import app

    assert app.version == __version__
