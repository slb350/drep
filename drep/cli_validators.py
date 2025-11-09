"""Custom Click parameter types for input validation.

This module provides reusable Click parameter types that enforce validation
at input time, ensuring users cannot enter invalid values that would fail
during configuration loading.
"""

import re
from urllib.parse import urlparse

import click


class URLType(click.ParamType):
    """Validates HTTP/HTTPS URL format.

    Ensures the URL has a valid scheme (http/https) and network location.
    Used for platform URLs and API endpoints.
    """

    name = "url"

    def convert(self, value, param, ctx):
        """Convert and validate URL string.

        Args:
            value: URL string to validate
            param: Click parameter (may be None)
            ctx: Click context (may be None)

        Returns:
            Original value if valid

        Raises:
            click.BadParameter: If URL is invalid
        """
        if not value:
            self.fail("URL cannot be empty", param, ctx)

        parsed = urlparse(value)

        # Check for required components
        if not parsed.scheme:
            self.fail(
                f"{value!r} is missing URL scheme (must start with http:// or https://)", param, ctx
            )

        if parsed.scheme not in ("http", "https"):
            self.fail(
                f"{value!r} is missing URL scheme (must start with http:// or https://)", param, ctx
            )

        if not parsed.netloc:
            self.fail(f"{value!r} is missing hostname", param, ctx)

        return value


class RepositoryListType(click.ParamType):
    """Validates comma-separated repository patterns.

    Ensures patterns follow the format:
    - owner/repo (specific repository)
    - owner/* (all repositories for owner)

    Filters out empty strings and validates format.
    """

    name = "repository-list"

    def convert(self, value, param, ctx):
        """Convert and validate repository pattern list.

        Args:
            value: Comma-separated repository patterns
            param: Click parameter (may be None)
            ctx: Click context (may be None)

        Returns:
            List of validated repository patterns

        Raises:
            click.BadParameter: If patterns are invalid
        """
        if not value:
            self.fail("Must provide at least one repository pattern", param, ctx)

        # Split and filter empty strings
        repos = [r.strip() for r in value.split(",")]
        repos = [r for r in repos if r]

        if not repos:
            self.fail("Must provide at least one repository pattern", param, ctx)

        # Validate each pattern: owner/repo or owner/*
        pattern = re.compile(r"^[a-zA-Z0-9_-]+/[a-zA-Z0-9_*-]+$")
        invalid = []

        for repo in repos:
            if not pattern.match(repo):
                invalid.append(repo)

        if invalid:
            examples = "\n".join(f"  - {r}" for r in invalid)
            self.fail(
                f"Invalid repository pattern(s):\n{examples}\n"
                f"Format must be 'owner/repo' or 'owner/*'",
                param,
                ctx,
            )

        return repos


class BedrockModelType(click.ParamType):
    """Validates AWS Bedrock model ID format.

    Ensures model ID starts with a valid provider prefix.
    Matches validation in drep.models.config.BedrockConfig.validate_model_id().
    """

    name = "bedrock-model"

    VALID_PREFIXES = [
        "anthropic.",
        "global.anthropic.",
        "amazon.",
        "global.amazon.",
        "meta.",
        "global.meta.",
        "cohere.",
        "global.cohere.",
    ]

    def convert(self, value, param, ctx):
        """Convert and validate Bedrock model ID.

        Args:
            value: Model ID string
            param: Click parameter (may be None)
            ctx: Click context (may be None)

        Returns:
            Original value if valid

        Raises:
            click.BadParameter: If model ID is invalid
        """
        if not value:
            self.fail("Model ID cannot be empty", param, ctx)

        if not any(value.startswith(prefix) for prefix in self.VALID_PREFIXES):
            prefixes_str = ", ".join(self.VALID_PREFIXES)
            self.fail(
                f"Invalid Bedrock model ID: {value!r}\n" f"Must start with one of: {prefixes_str}",
                param,
                ctx,
            )

        return value


class DatabaseURLType(click.ParamType):
    """Validates database URL format.

    Ensures URL contains '://' and has a recognized scheme.
    Basic validation for SQLAlchemy database URLs.
    """

    name = "database-url"

    KNOWN_SCHEMES = ["sqlite", "postgresql", "mysql", "mariadb"]

    def convert(self, value, param, ctx):
        """Convert and validate database URL.

        Args:
            value: Database URL string
            param: Click parameter (may be None)
            ctx: Click context (may be None)

        Returns:
            Original value if valid

        Raises:
            click.BadParameter: If URL is invalid
        """
        if not value:
            self.fail("Database URL cannot be empty", param, ctx)

        if "://" not in value:
            self.fail(
                f"{value!r} is not a valid database URL\n"
                f"Must contain '://' (e.g., sqlite:///./drep.db)",
                param,
                ctx,
            )

        scheme = value.split("://")[0]

        if scheme not in self.KNOWN_SCHEMES:
            # Warn but don't fail for unknown schemes
            click.echo(
                f"Warning: Unrecognized database scheme {scheme!r}. "
                f"Known schemes: {', '.join(self.KNOWN_SCHEMES)}",
                err=True,
            )

            if not click.confirm("Continue anyway?", default=False):
                self.fail("Database URL validation cancelled by user", param, ctx)

        return value


class NonEmptyString(click.ParamType):
    """Validates that string is not empty after stripping whitespace.

    Used for model names, regions, and other required string values.
    """

    name = "non-empty-string"

    def convert(self, value, param, ctx):
        """Convert and validate non-empty string.

        Args:
            value: String to validate
            param: Click parameter (may be None)
            ctx: Click context (may be None)

        Returns:
            Stripped string if valid

        Raises:
            click.BadParameter: If string is empty
        """
        if not value or not value.strip():
            self.fail("Value cannot be empty", param, ctx)

        return value.strip()
