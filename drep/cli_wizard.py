"""Interactive configuration collection for the init wizard.

Each _collect_* function prompts for one config section and returns the
strongly-typed wizard model; _write_and_validate_config serializes and
validates the assembled config.
"""

from typing import Any

import click
import yaml
from pydantic import ValidationError

from drep.cli_validators import (
    BedrockModelType,
    DatabaseURLType,
    NonEmptyString,
    RepositoryListType,
    URLType,
)
from drep.config import load_config
from drep.models.llm_presets import LLM_PRESETS
from drep.models.wizard import (
    BedrockLLMData,
    BedrockRegionModel,
    DocumentationConfig,
    DocumentationConfigData,
    GiteaPlatformData,
    GitHubPlatformData,
    GitLabPlatformData,
    LLMConfig,
    OpenAILLMData,
    PlatformConfig,
)

# Single source of truth for the wizard's LLM defaults. Used both as the
# click.prompt defaults and as the values written when the user declines to
# configure advanced settings.
_LLM_DEFAULTS: dict[str, Any] = {
    "temperature": 0.2,
    "max_tokens": 8000,
    "timeout": 60,
    "max_retries": 3,
    "retry_delay": 2,
    "exponential_backoff": True,
    "max_concurrent_global": 5,
    "max_concurrent_per_repo": 3,
    "requests_per_minute": 60,
    "max_tokens_per_minute": 100000,
}

_CACHE_DEFAULTS: dict[str, Any] = {"enabled": True, "ttl_days": 30, "max_size_gb": 10.0}


def _collect_platform_config() -> PlatformConfig:
    """Collect platform configuration from user.

    Returns:
        PlatformConfig containing platform dict, env var, and platform name
    """
    click.echo("Step 1: Git Platform Configuration")
    click.echo("-" * 60)
    platform = click.prompt(
        "Which git platform are you using?",
        type=click.Choice(["github", "gitea", "gitlab"], case_sensitive=False),
        default="github",
    )
    click.echo()

    if platform.lower() == "github":
        click.echo("GitHub Configuration:")
        use_enterprise = click.confirm("Are you using GitHub Enterprise?", default=False)

        # Collect URL if enterprise
        api_url = None
        if use_enterprise:
            api_url = click.prompt(
                "GitHub Enterprise API URL",
                default="https://github.example.com/api/v3",
                type=URLType(),
            )

        click.echo("\nRepository Configuration:")
        click.echo("Examples: 'your-org/*' (all repos), 'owner/repo' (single repo)")
        repos = click.prompt(
            "Enter repositories (comma-separated)", default="your-org/*", type=RepositoryListType()
        )

        # Create strongly-typed data
        github_data = GitHubPlatformData(
            token="${GITHUB_TOKEN}",
            repositories=tuple(repos),  # Convert list to tuple
            url=api_url,  # None for github.com, URL for enterprise
        )

        return PlatformConfig(data=github_data)

    if platform.lower() == "gitea":
        click.echo("Gitea Configuration:")

        gitea_url = click.prompt("Gitea URL", default="http://localhost:3000", type=URLType())

        click.echo("\nRepository Configuration:")
        click.echo("Examples: 'your-org/*' (all repos), 'owner/repo' (single repo)")
        repos = click.prompt(
            "Enter repositories (comma-separated)", default="your-org/*", type=RepositoryListType()
        )

        # Create strongly-typed data
        gitea_data = GiteaPlatformData(
            url=gitea_url,
            token="${GITEA_TOKEN}",
            repositories=tuple(repos),  # Convert list to tuple
        )

        return PlatformConfig(data=gitea_data)

    click.echo("GitLab Configuration:")
    use_selfhosted = click.confirm("Are you using self-hosted GitLab?", default=False)

    # Collect URL if self-hosted
    gitlab_url = None
    if use_selfhosted:
        gitlab_url = click.prompt(
            "GitLab URL", default="https://gitlab.example.com", type=URLType()
        )

    click.echo("\nRepository Configuration:")
    click.echo("Examples: 'your-org/*' (all projects), 'owner/project' (single project)")
    repos = click.prompt(
        "Enter projects (comma-separated)", default="your-org/*", type=RepositoryListType()
    )

    # Create strongly-typed data
    gitlab_data = GitLabPlatformData(
        token="${GITLAB_TOKEN}",
        repositories=tuple(repos),  # Convert list to tuple
        url=gitlab_url,  # None for gitlab.com, URL for self-hosted
    )

    return PlatformConfig(data=gitlab_data)


def _collect_llm_config() -> LLMConfig | None:
    """Collect LLM configuration from user.

    Returns:
        LLMConfig if LLM is enabled, None if disabled
    """
    click.echo("Step 2: LLM Configuration")
    click.echo("-" * 60)
    llm_enabled = click.confirm("Enable LLM-powered code analysis?", default=True)
    click.echo()

    if not llm_enabled:
        return None

    click.echo("LLM Provider Options:")
    click.echo("  1. openai-compatible - Use local LLM (LM Studio, Ollama, etc.)")
    click.echo("  2. bedrock - AWS Bedrock")

    provider = click.prompt(
        "Choose provider",
        type=click.Choice(["openai-compatible", "bedrock"], case_sensitive=False),
        default="openai-compatible",
    )

    llm_config = {"enabled": True, "provider": provider}

    if provider == "openai-compatible":
        click.echo("\nOpenAI-Compatible Configuration:")
        # Defaults come from the shared preset table, so `drep init` and
        # `drep init-llm --provider local` cannot drift apart.
        local = LLM_PRESETS["local"]
        endpoint = click.prompt("API Endpoint", default=local.endpoint, type=URLType())
        model = click.prompt("Model name", default=local.default_model, type=NonEmptyString())
        llm_config["endpoint"] = endpoint
        llm_config["model"] = model

        use_api_key = click.confirm("Does your endpoint require an API key?", default=False)
        if use_api_key:
            llm_config["api_key"] = "${LLM_API_KEY}"

    elif provider == "bedrock":
        click.echo("\nAWS Bedrock Configuration:")
        region = click.prompt("AWS Region", default="us-east-1", type=NonEmptyString())
        model = click.prompt(
            "Bedrock Model ID",
            default="anthropic.claude-sonnet-4-5-20250929-v1:0",
            type=BedrockModelType(),
        )
        llm_config["bedrock"] = BedrockRegionModel(region=region, model=model)

    click.echo()

    configure_advanced = click.confirm("Configure advanced LLM settings?", default=False)

    # Start from the defaults unconditionally, then overwrite only what the user
    # was actually asked. Previously the prompted and unprompted branches each
    # spelled out all ten values, so a changed default had to be edited twice
    # and the two copies were only coincidentally equal.
    llm_config.update(_LLM_DEFAULTS)

    if configure_advanced:
        click.echo("\nAdvanced LLM Settings:")
        llm_config["temperature"] = click.prompt(
            "Temperature (0.0-2.0)",
            default=_LLM_DEFAULTS["temperature"],
            type=click.FloatRange(min=0.0, max=2.0),
        )
        llm_config["max_tokens"] = click.prompt(
            "Max tokens per request",
            default=_LLM_DEFAULTS["max_tokens"],
            type=click.IntRange(min=100, max=20000),
        )
        llm_config["timeout"] = click.prompt(
            "Request timeout (seconds)",
            default=_LLM_DEFAULTS["timeout"],
            type=click.IntRange(min=10, max=300),
        )
        llm_config["max_retries"] = click.prompt(
            "Max retries on failure",
            default=_LLM_DEFAULTS["max_retries"],
            type=click.IntRange(min=0, max=10),
        )
        llm_config["max_concurrent_global"] = click.prompt(
            "Max concurrent requests (global)",
            default=_LLM_DEFAULTS["max_concurrent_global"],
            type=click.IntRange(min=1, max=50),
        )
        llm_config["requests_per_minute"] = click.prompt(
            "Requests per minute limit",
            default=_LLM_DEFAULTS["requests_per_minute"],
            type=click.IntRange(min=1, max=1000),
        )

    click.echo()

    configure_cache = click.confirm("Configure LLM response caching?", default=False)

    cache = dict(_CACHE_DEFAULTS)
    if configure_cache:
        click.echo("\nCache Settings:")
        cache["enabled"] = click.confirm("Enable cache?", default=_CACHE_DEFAULTS["enabled"])
        cache["ttl_days"] = click.prompt(
            "Cache TTL (days)", default=_CACHE_DEFAULTS["ttl_days"], type=click.IntRange(min=1)
        )
        cache["max_size_gb"] = click.prompt(
            "Max cache size (GB)",
            default=_CACHE_DEFAULTS["max_size_gb"],
            type=click.FloatRange(min=0.1),
        )
    llm_config["cache"] = cache

    # Create strongly-typed data model based on provider
    if provider == "openai-compatible":
        return LLMConfig(data=OpenAILLMData(**llm_config))
    return LLMConfig(data=BedrockLLMData(**llm_config))


def _collect_documentation_config() -> DocumentationConfig:
    """Collect documentation configuration from user.

    Returns:
        DocumentationConfig containing documentation settings
    """
    click.echo("Step 3: Documentation Analysis")
    click.echo("-" * 60)
    doc_enabled = click.confirm("Enable documentation analysis?", default=True)

    markdown_checks = False
    words_tuple = ()

    if doc_enabled:
        markdown_checks = click.confirm("Enable markdown lint checks?", default=False)

        custom_dict = click.confirm("Add custom dictionary words?", default=False)
        if custom_dict:
            words = click.prompt("Enter words (comma-separated)", default="")
            # Filter out empty/whitespace-only entries and convert to tuple
            words_list = [w.strip() for w in words.split(",") if w.strip()]
            words_tuple = tuple(words_list)

    # Create strongly-typed data
    doc_data = DocumentationConfigData(
        enabled=doc_enabled,
        markdown_checks=markdown_checks,
        custom_dictionary=words_tuple,
    )

    click.echo()
    return DocumentationConfig(data=doc_data)


def _collect_database_config():
    """Collect database configuration from user.

    Returns:
        Database URL string
    """
    click.echo("Step 4: Database Configuration")
    click.echo("-" * 60)
    use_custom_db = click.confirm("Use custom database URL?", default=False)

    if use_custom_db:
        db_url = click.prompt("Database URL", default="sqlite:///./drep.db", type=DatabaseURLType())
    else:
        db_url = "sqlite:///./drep.db"

    click.echo()
    return db_url


def _write_and_validate_config(config_dict, config_path):
    """Write configuration to file and validate it.

    Args:
        config_dict: Configuration dictionary
        config_path: Path to write config file

    Raises:
        click.Abort: If validation fails or file cannot be written
    """
    try:
        config_yaml = yaml.dump(config_dict, default_flow_style=False, sort_keys=False)
    except yaml.YAMLError as e:
        click.echo(f"ERROR: Failed to serialize configuration: {e}", err=True)
        click.echo("This is a bug. Please report this issue.", err=True)
        raise click.Abort() from e

    try:
        config_path.write_text(config_yaml)
    except PermissionError as exc:
        click.echo(f"ERROR: Permission denied writing to {config_path}", err=True)
        click.echo("Check file permissions.", err=True)
        raise click.Abort() from exc
    except OSError as e:
        click.echo(f"ERROR: Failed to write config: {e}", err=True)
        click.echo("Check disk space and permissions.", err=True)
        raise click.Abort() from e

    click.echo("=" * 60)
    click.echo("Validating configuration...")
    click.echo("-" * 60)

    try:
        load_config(str(config_path), strict=False)
        click.echo("✓ Configuration structure is valid!")
    except ValidationError as e:
        click.echo("ERROR: Configuration validation failed:", err=True)
        for error in e.errors():
            field = " -> ".join(str(x) for x in error["loc"])
            click.echo(f"  - {field}: {error['msg']}", err=True)
        click.echo(f"\nConfig file: {config_path}", err=True)
        click.echo("Please re-run 'drep init' or fix manually.", err=True)
        raise click.Abort() from e
    except ValueError as e:
        click.echo(f"ERROR: Configuration validation failed: {e}", err=True)
        click.echo(f"\nConfig file: {config_path}", err=True)
        click.echo("Please re-run 'drep init' or fix manually.", err=True)
        raise click.Abort() from e

    # SECURITY: Catch only specific, recoverable exceptions
    # This code catches ValidationError (Pydantic schema failures) and ValueError
    # (YAML parsing errors) to provide user-friendly error messages. All other
    # exceptions propagate naturally:
    # - KeyboardInterrupt: Allows user to interrupt wizard (Ctrl+C)
    # - MemoryError: Signals resource exhaustion to calling process
    # - ImportError: Reports missing dependencies with full traceback
    # - RuntimeError: Exposes unexpected errors for debugging
    # This selective exception handling provides helpful feedback for expected
    # errors while preserving full diagnostic information for unexpected failures.
