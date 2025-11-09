"""CLI interface for drep."""

import asyncio
import os
import shutil
import tempfile
from enum import Enum
from pathlib import Path
from typing import Optional

import click
import yaml
from git import GitCommandError, InvalidGitRepositoryError, Repo
from pydantic_core import ValidationError

from drep.adapters.gitea import GiteaAdapter
from drep.cli_validators import (
    BedrockModelType,
    DatabaseURLType,
    NonEmptyString,
    RepositoryListType,
    URLType,
)
from drep.config import find_config_file, get_user_config_dir, load_config
from drep.core.issue_manager import IssueManager
from drep.core.scanner import RepositoryScanner
from drep.db import init_database
from drep.documentation.analyzer import DocumentationAnalyzer
from drep.models.wizard import (
    AnthropicLLMData,
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


class OutputFormat(str, Enum):
    """Output format options for check command."""

    TEXT = "text"
    JSON = "json"


@click.group()
def cli():
    """drep - Documentation & Review Enhancement Platform"""
    pass


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

        return PlatformConfig(data=github_data, env_var="GITHUB_TOKEN", platform_name="GitHub")

    elif platform.lower() == "gitea":
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

        return PlatformConfig(data=gitea_data, env_var="GITEA_TOKEN", platform_name="Gitea")

    else:
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

        return PlatformConfig(data=gitlab_data, env_var="GITLAB_TOKEN", platform_name="GitLab")


def _collect_llm_config() -> Optional[LLMConfig]:
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
    click.echo("  3. anthropic - Anthropic API (Claude)")

    provider = click.prompt(
        "Choose provider",
        type=click.Choice(["openai-compatible", "bedrock", "anthropic"], case_sensitive=False),
        default="openai-compatible",
    )

    llm_config = {"enabled": True, "provider": provider}

    if provider == "openai-compatible":
        click.echo("\nOpenAI-Compatible Configuration:")
        endpoint = click.prompt("API Endpoint", default="http://localhost:1234/v1", type=URLType())
        model = click.prompt("Model name", default="qwen3-30b-a3b", type=NonEmptyString())
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

    elif provider == "anthropic":
        click.echo("\nAnthropic Configuration:")
        llm_config["api_key"] = "${ANTHROPIC_API_KEY}"
        model = click.prompt(
            "Model name", default="claude-sonnet-4-5-20250929", type=NonEmptyString()
        )
        llm_config["model"] = model

    click.echo()

    configure_advanced = click.confirm("Configure advanced LLM settings?", default=False)

    if configure_advanced:
        click.echo("\nAdvanced LLM Settings:")
        temperature = click.prompt(
            "Temperature (0.0-2.0)", default=0.2, type=click.FloatRange(min=0.0, max=2.0)
        )
        max_tokens = click.prompt(
            "Max tokens per request", default=8000, type=click.IntRange(min=100, max=20000)
        )
        timeout = click.prompt(
            "Request timeout (seconds)", default=60, type=click.IntRange(min=10, max=300)
        )
        max_retries = click.prompt(
            "Max retries on failure", default=3, type=click.IntRange(min=0, max=10)
        )
        max_concurrent = click.prompt(
            "Max concurrent requests (global)", default=5, type=click.IntRange(min=1, max=50)
        )
        requests_per_min = click.prompt(
            "Requests per minute limit", default=60, type=click.IntRange(min=1, max=1000)
        )

        llm_config.update(
            {
                "temperature": temperature,
                "max_tokens": max_tokens,
                "timeout": timeout,
                "max_retries": max_retries,
                "retry_delay": 2,
                "exponential_backoff": True,
                "max_concurrent_global": max_concurrent,
                "max_concurrent_per_repo": 3,
                "requests_per_minute": requests_per_min,
                "max_tokens_per_minute": 100000,
            }
        )
    else:
        llm_config.update(
            {
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
        )

    click.echo()

    configure_cache = click.confirm("Configure LLM response caching?", default=False)

    if configure_cache:
        click.echo("\nCache Settings:")
        cache_enabled = click.confirm("Enable cache?", default=True)
        ttl_days = click.prompt("Cache TTL (days)", default=30, type=click.IntRange(min=1))
        max_size_gb = click.prompt(
            "Max cache size (GB)", default=10.0, type=click.FloatRange(min=0.1)
        )

        llm_config["cache"] = {
            "enabled": cache_enabled,
            "ttl_days": ttl_days,
            "max_size_gb": max_size_gb,
        }
    else:
        llm_config["cache"] = {"enabled": True, "ttl_days": 30, "max_size_gb": 10.0}

    # Create strongly-typed data model based on provider
    if provider == "openai-compatible":
        llm_data = OpenAILLMData(**llm_config)
        return LLMConfig(data=llm_data)
    elif provider == "bedrock":
        llm_data = BedrockLLMData(**llm_config)
        return LLMConfig(data=llm_data)
    else:  # anthropic
        llm_data = AnthropicLLMData(**llm_config)
        return LLMConfig(data=llm_data)


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
        raise click.Abort()

    try:
        config_path.write_text(config_yaml)
    except PermissionError:
        click.echo(f"ERROR: Permission denied writing to {config_path}", err=True)
        click.echo("Check file permissions.", err=True)
        raise click.Abort()
    except OSError as e:
        click.echo(f"ERROR: Failed to write config: {e}", err=True)
        click.echo("Check disk space and permissions.", err=True)
        raise click.Abort()

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
        raise click.Abort()
    except ValueError as e:
        click.echo(f"ERROR: Configuration validation failed: {e}", err=True)
        click.echo(f"\nConfig file: {config_path}", err=True)
        click.echo("Please re-run 'drep init' or fix manually.", err=True)
        raise click.Abort()

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


@cli.command()
def init():
    """Initialize drep configuration with interactive setup wizard.

    Guides the user through a multi-step wizard to configure:
    1. Configuration location (current directory or user config directory)
    2. Platform selection (GitHub/Gitea/GitLab) with platform-specific options
    3. LLM configuration (optional) - supports OpenAI-compatible, Bedrock, Anthropic
    4. Documentation analysis settings (markdown checks, custom dictionary)
    5. Database configuration (SQLite, PostgreSQL, MySQL, etc.)
    6. Environment variable verification (optional)

    Creates config.yaml in the chosen location. If the file already exists,
    creates a backup (.yaml.backup) before overwriting. All inputs are validated
    at entry time using custom Click validators to prevent invalid configurations.

    Error Handling:
    - Backup failures abort the wizard to prevent data loss
    - File write errors (PermissionError, OSError) show clear error messages
    - YAML serialization errors are caught and reported
    - Config validation failures show detailed field-level errors
    - Environment variable checks wrapped in try-except for restricted environments

    Security:
    - Never stores secrets in config (uses ${ENV_VAR} placeholders)
    - All validators enforce strict format requirements
    - Optional environment variable verification to catch missing credentials
    - Backup mechanism prevents accidental config loss

    Exit Codes:
    - 0: Configuration created and validated successfully
    - 1: User aborted, validation failed, or unrecoverable error occurred

    Raises:
        click.Abort: If backup creation fails, file cannot be written, or validation fails
    """
    click.echo("=" * 60)
    click.echo("Welcome to drep configuration setup!")
    click.echo("=" * 60)
    click.echo()

    # Prompt for config location
    click.echo("Where should the configuration be created?")
    click.echo()
    click.echo("  1. Current directory (./config.yaml)")
    click.echo("     Use for project-specific configuration")
    click.echo()
    user_config_dir = get_user_config_dir()
    click.echo(f"  2. User config directory ({user_config_dir}/config.yaml)")
    click.echo("     Use for system-wide configuration (recommended for pip/brew install)")
    click.echo()

    location_choice = click.prompt(
        "Choose location",
        type=click.Choice(["1", "2"], case_sensitive=False),
        default="2",
    )

    if location_choice == "1":
        config_path = Path("config.yaml")
    else:
        config_path = user_config_dir / "config.yaml"
        # Create directory if it doesn't exist
        try:
            config_path.parent.mkdir(parents=True, exist_ok=True)
        except PermissionError:
            click.echo(f"ERROR: Cannot create directory {config_path.parent}", err=True)
            click.echo("  Permission denied. Try using location 1 (current directory).", err=True)
            raise click.Abort()
        except OSError as e:
            click.echo(f"ERROR: Cannot create directory: {e}", err=True)
            raise click.Abort()

    # Check if config already exists
    if config_path.exists():
        click.echo()
        if click.confirm(f"{config_path} already exists. Overwrite?", default=False):
            # SECURITY: Create backup before overwriting existing config
            # This copies the existing config.yaml to config.yaml.backup before proceeding.
            # If the backup creation fails due to permissions or filesystem errors, the
            # wizard aborts (raises click.Abort) to preserve the existing configuration.
            # This ensures users can recover their previous settings if the new config
            # write fails or produces an invalid configuration.
            backup_path = config_path.with_suffix(".yaml.backup")
            try:
                shutil.copy(config_path, backup_path)
                click.echo(f"Backup created: {backup_path}")
            except PermissionError:
                click.echo(
                    f"ERROR: Cannot create backup at {backup_path}\n"
                    f"Permission denied. Cannot safely overwrite config.",
                    err=True,
                )
                raise click.Abort()
            except OSError as e:
                click.echo(
                    f"ERROR: Cannot create backup: {e}\n"
                    f"Cannot safely overwrite config without backup.",
                    err=True,
                )
                raise click.Abort()
            click.echo()
        else:
            raise click.Abort()

    click.echo()

    platform_config = _collect_platform_config()
    llm_config = _collect_llm_config()
    doc_config = _collect_documentation_config()
    db_url = _collect_database_config()

    config_dict = {}
    config_dict.update(platform_config.to_dict())  # Use to_dict() for new strongly-typed format
    config_dict.update(doc_config.to_dict())  # Use to_dict() for new strongly-typed format
    config_dict["database_url"] = db_url
    if llm_config is not None:
        config_dict.update(llm_config.to_dict())  # Use to_dict() for new strongly-typed format

    _write_and_validate_config(config_dict, config_path)

    click.echo()
    click.echo("=" * 60)
    click.echo("✓ Configuration created successfully!")
    click.echo("=" * 60)
    click.echo(f"\nConfig location: {config_path}")
    click.echo("\nNext steps:")
    click.echo(f"1. Set the {platform_config.env_var} environment variable:")
    click.echo(f"   export {platform_config.env_var}='your-api-token-here'")

    if (
        llm_config
        and llm_config.provider == "openai-compatible"
        and "${LLM_API_KEY}" in yaml.dump(config_dict)
    ):
        click.echo("   export LLM_API_KEY='your-llm-api-key'")
    elif llm_config and llm_config.provider == "anthropic":
        click.echo("   export ANTHROPIC_API_KEY='your-anthropic-api-key'")
    elif llm_config and llm_config.provider == "bedrock":
        click.echo("   Configure AWS credentials (AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY)")

    if click.confirm("\nCheck if required environment variables are set?", default=False):
        # SECURITY: Check environment variables with error handling
        # This code reads required tokens from os.environ to verify they're set before
        # the user runs a scan. It catches OSError and PermissionError (restricted
        # environments like containers/sandboxes may restrict os.environ access) and
        # displays a warning. KeyboardInterrupt propagates naturally to allow wizard abort.
        try:
            missing = []
            if platform_config.env_var not in os.environ:
                missing.append(platform_config.env_var)
            if (
                llm_config
                and llm_config.provider == "anthropic"
                and "ANTHROPIC_API_KEY" not in os.environ
            ):
                missing.append("ANTHROPIC_API_KEY")
            if (
                llm_config
                and llm_config.provider == "openai-compatible"
                and "${LLM_API_KEY}" in yaml.dump(config_dict)
                and "LLM_API_KEY" not in os.environ
            ):
                missing.append("LLM_API_KEY")
            if llm_config and llm_config.provider == "bedrock":
                if "AWS_ACCESS_KEY_ID" not in os.environ:
                    missing.append("AWS_ACCESS_KEY_ID")
                if "AWS_SECRET_ACCESS_KEY" not in os.environ:
                    missing.append("AWS_SECRET_ACCESS_KEY")

            if missing:
                click.echo("WARNING: Missing environment variables:", err=True)
                for var in missing:
                    click.echo(f"  - {var}", err=True)
            else:
                click.echo("✓ All required environment variables are set!")
        except (OSError, PermissionError) as e:
            import logging

            logger = logging.getLogger(__name__)
            logger.debug(f"Cannot access environment variables: {e}")
            click.echo(
                "WARNING: Cannot check environment variables in this environment.",
                err=True,
            )
            click.echo("Please verify manually that required tokens are set.", err=True)
        # KeyboardInterrupt, MemoryError, and other exceptions propagate naturally

    click.echo("\n2. Validate your configuration:")
    click.echo("   drep validate")

    click.echo("\n3. Start scanning repositories:")
    click.echo("   drep scan owner/repo")

    click.echo("\n4. (Optional) Review a pull request:")
    click.echo("   drep review owner/repo PR_NUMBER")
    click.echo()


@cli.command()
@click.argument("repository")
@click.option("--config", default=None, help="Config file path (optional, auto-discovers)")
@click.option("--show-metrics/--no-metrics", default=False, help="Show LLM metrics after scan")
@click.option("--show-progress/--no-progress", default=True, help="Show progress during scan")
def scan(repository, config, show_metrics, show_progress):
    """Scan a repository: drep scan owner/repo"""

    if "/" not in repository:
        click.echo("Error: Repository must be in format 'owner/repo'", err=True)
        return

    owner, repo_name = repository.split("/", 1)

    # Discover config file
    config_path = find_config_file(config)
    if not config_path.exists():
        click.echo(f"Config file not found: {config_path}", err=True)
        click.echo("Run 'drep init' to create a config file.", err=True)
        return

    click.echo(f"Scanning {owner}/{repo_name}...")

    try:
        # Run async scan
        asyncio.run(_run_scan(owner, repo_name, str(config_path), show_metrics, show_progress))
        click.echo("✓ Scan complete")
    except click.Abort:
        # Re-raise to let Click handle the abort (already displayed error message)
        raise
    except FileNotFoundError:
        click.echo(f"Config file not found: {config_path}", err=True)
        click.echo("Run 'drep init' to create a config file.", err=True)
    except Exception as e:
        click.echo(f"Error during scan: {e}", err=True)


async def _run_scan(
    owner: str,
    repo: str,
    config_path: str,
    show_metrics: bool,
    show_progress: bool,
):
    """Run the actual scan workflow.

    Args:
        owner: Repository owner
        repo: Repository name
        config_path: Path to config file
        show_metrics: Whether to show LLM metrics after scan
        show_progress: Whether to show progress during scan
    """
    # Load config
    config = load_config(config_path)

    # Determine which adapter to use (prefer Gitea for backward compatibility)
    platform = None
    adapter = None
    git_url = None
    git_token = None

    if config.gitea is not None:
        # Use Gitea adapter
        platform = "gitea"
        adapter = GiteaAdapter(config.gitea.url, config.gitea.token.get_secret_value())
        git_url = f"{config.gitea.url.rstrip('/')}/{owner}/{repo}.git"
        git_token = config.gitea.token.get_secret_value()
    elif config.github is not None:
        # Use GitHub adapter
        platform = "github"
        from drep.adapters.github import GitHubAdapter

        adapter = GitHubAdapter(
            token=config.github.token.get_secret_value(),
            url=str(config.github.url) if config.github.url else "https://api.github.com",
        )
        # GitHub git URL format: https://github.com/owner/repo.git
        # Handle default GitHub.com case (config.github.url is None)
        if config.github.url is None or "github.com" in str(config.github.url):
            git_url = f"https://github.com/{owner}/{repo}.git"
        else:
            # GitHub Enterprise - extract hostname from API URL
            api_url = str(config.github.url)
            hostname = api_url.replace("https://", "").replace("http://", "").split("/")[0]
            git_url = f"https://{hostname}/{owner}/{repo}.git"
        git_token = config.github.token.get_secret_value()
    elif config.gitlab is not None:
        # Use GitLab adapter
        platform = "gitlab"
        from drep.adapters.gitlab import GitLabAdapter

        adapter = GitLabAdapter(
            token=config.gitlab.token.get_secret_value(),
            url=config.gitlab.url,  # None for GitLab.com, or custom URL
        )
        # GitLab git URL format: https://gitlab.com/owner/repo.git
        # Handle default GitLab.com case (config.gitlab.url is None)
        if config.gitlab.url is None:
            git_url = f"https://gitlab.com/{owner}/{repo}.git"
        else:
            # Self-hosted GitLab
            git_url = f"{config.gitlab.url.rstrip('/')}/{owner}/{repo}.git"
        git_token = config.gitlab.token.get_secret_value()
    else:
        # No platform configured (shouldn't happen - Config validator requires at least one)
        click.echo(
            "Error: No platform configured. "
            "Please add [gitea], [github], or [gitlab] to your config.yaml.",
            err=True,
        )
        raise click.Abort()

    # Initialize components
    session = init_database(config.database_url)
    scanner = RepositoryScanner(session, config)  # Pass config for LLM support
    analyzer = DocumentationAnalyzer(config.documentation)
    issue_manager = IssueManager(adapter, session)

    # Temporary directory for askpass script
    temp_dir = None

    try:
        # Setup git authentication
        temp_dir = tempfile.mkdtemp(prefix="drep_git_")
        askpass_script = Path(temp_dir) / "askpass.sh"
        token_file = Path(temp_dir) / ".git-token"

        # SECURITY: Write token to file instead of environment variable
        # Threat model: Process listings (ps, /proc) can expose environment variables
        # to other users on the system. File-based authentication with chmod 0o600 ensures
        # only the current user (process owner) can read the token, preventing exposure
        # in multi-user environments. The askpass script reads from this file securely.
        token_file.write_text(git_token)
        token_file.chmod(0o600)  # Owner read/write only

        # Create askpass script that reads token from file
        askpass_content = f"""#!/bin/sh
if echo "$1" | grep -qi "username"; then
    echo "token"
elif echo "$1" | grep -qi "password"; then
    cat {token_file}
else
    cat {token_file}
fi
"""
        askpass_script.write_text(askpass_content)
        # Restrict to owner only; contains sensitive file path
        askpass_script.chmod(0o700)

        # Build git environment (no token in environment!)
        git_env = {
            **os.environ,
            "GIT_ASKPASS": str(askpass_script),
            "GIT_TERMINAL_PROMPT": "0",
        }

        # Repository path
        repo_path = Path("./repos") / owner / repo

        # Clone or pull repository
        try:
            if not repo_path.exists():
                click.echo(f"Cloning {platform} repository...")
                repo_path.parent.mkdir(parents=True, exist_ok=True)

                # Get default branch
                try:
                    default_branch = await adapter.get_default_branch(owner, repo)
                except Exception as e:
                    import logging

                    logger = logging.getLogger(__name__)
                    logger.error(f"Failed to get default branch for {owner}/{repo}: {e}")
                    click.echo(f"Error: Cannot access repository {owner}/{repo}", err=True)
                    click.echo("  Check that repository exists and token has access.", err=True)
                    raise click.Abort()

                if not default_branch:
                    click.echo(f"Error: Repository {owner}/{repo} has no branches", err=True)
                    raise click.Abort()

                # Clone
                try:
                    Repo.clone_from(git_url, repo_path, branch=default_branch, env=git_env)
                except GitCommandError as e:
                    error_msg = str(e).lower()
                    if "authentication failed" in error_msg:
                        # Determine which token env var to suggest
                        token_env_var = (
                            "GITHUB_TOKEN"
                            if platform == "github"
                            else "GITEA_TOKEN" if platform == "gitea" else "GITLAB_TOKEN"
                        )
                        click.echo("Error: Authentication failed", err=True)
                        click.echo(f"  Check your {token_env_var} token is valid", err=True)
                    elif "not found" in error_msg:
                        click.echo(f"Error: Repository {owner}/{repo} not found", err=True)
                        click.echo("  Verify repository exists and token has access", err=True)
                    else:
                        click.echo(f"Error: Git clone failed: {e}", err=True)
                    raise click.Abort()
            else:
                click.echo("Pulling latest changes...")
                try:
                    git_repo = Repo(repo_path)
                    with git_repo.git.custom_environment(**git_env):
                        git_repo.remotes.origin.pull()
                except GitCommandError as e:
                    import logging

                    logger = logging.getLogger(__name__)
                    logger.error(f"Git pull failed for {owner}/{repo}: {e}")
                    click.echo(f"Error: Git pull failed: {e}", err=True)
                    click.echo(f"  Try: rm -rf {repo_path} to force re-clone", err=True)
                    raise click.Abort()
                except InvalidGitRepositoryError:
                    import logging

                    logger = logging.getLogger(__name__)
                    logger.error(f"Corrupted git repository at {repo_path}")
                    click.echo(f"Error: Corrupted git repository at {repo_path}", err=True)
                    click.echo(f"  Fix: rm -rf {repo_path} and re-run scan", err=True)
                    raise click.Abort()
        except PermissionError as e:
            import logging

            logger = logging.getLogger(__name__)
            logger.error(f"Permission denied accessing {repo_path}: {e}")
            click.echo(f"Error: Cannot write to {repo_path}", err=True)
            click.echo("  Check directory permissions", err=True)
            raise click.Abort()

        # Scan repository
        click.echo("Analyzing files...")
        files, current_sha = await scanner.scan_repository(str(repo_path), owner, repo)

        if current_sha is None:
            click.echo("Repository has no commits yet. Skipping.", err=True)
            return

        if not files:
            click.echo("No files to analyze.")
        else:
            click.echo(f"Analyzing {len(files)} files...")

        # Progress callback for real-time updates
        def update_progress(tracker):
            """Update progress display in terminal."""
            if show_progress:
                # Use \r for in-place updates, no newline
                click.echo(f"\r{tracker.report()}", nl=False)

        # Analyze files and collect findings
        findings = []

        # 1. Documentation analysis (legacy)
        for file_path in files:
            full_path = Path(repo_path) / file_path
            if full_path.exists():
                try:
                    content = full_path.read_text(encoding="utf-8")
                    result = await analyzer.analyze_file(file_path, content)
                    findings.extend(result.to_findings())
                except UnicodeDecodeError:
                    click.echo(f"Warning: Skipping {file_path}: Not valid UTF-8", err=True)
                    continue
                except PermissionError:
                    click.echo(f"Error: Permission denied: {file_path}", err=True)
                    continue
                except OSError as e:
                    click.echo(f"Error: Failed to read {file_path}: {e}", err=True)
                    continue

        # 2. Code quality analysis (LLM-powered)
        if config.llm and config.llm.enabled:
            click.echo("Analyzing code quality...")
            repo_id = f"{owner}/{repo}"
            code_findings = await scanner.analyze_code_quality(
                repo_path=str(repo_path),
                files=files,
                repo_id=repo_id,
                commit_sha=current_sha,
                progress_callback=update_progress if show_progress else None,
            )

            if show_progress:
                click.echo("")  # New line after progress bar completes

            findings.extend(code_findings)

            # 3. Docstring analysis (LLM-powered)
            click.echo("Analyzing docstrings...")
            docstring_findings = await scanner.analyze_docstrings(
                repo_path=str(repo_path),
                files=files,
                repo_id=repo_id,
                commit_sha=current_sha,
                progress_callback=update_progress if show_progress else None,
            )

            if show_progress:
                click.echo("")  # New line after progress bar completes

            findings.extend(docstring_findings)

        click.echo(f"Found {len(findings)} issues")

        # Create issues
        if findings:
            await issue_manager.create_issues_for_findings(owner, repo, findings)

        # Record scan
        scanner.record_scan(owner, repo, current_sha)

        # Persist and/or show metrics at end
        if scanner.llm_client:
            metrics = scanner.llm_client.get_llm_metrics()

            # Save metrics to ~/.drep/metrics.json
            try:
                import logging
                from pathlib import Path as _Path

                from drep.llm.metrics import MetricsCollector

                metrics_file = _Path.home() / ".drep" / "metrics.json"
                metrics_file.parent.mkdir(parents=True, exist_ok=True)
                collector = MetricsCollector(metrics_file)
                collector.current_session = metrics
                await collector.save()
            except PermissionError:
                import logging

                logger = logging.getLogger(__name__)
                logger.error(f"Permission denied writing metrics to {metrics_file}")
                click.echo(f"Warning: Cannot save metrics to {metrics_file}", err=True)
                click.echo(f"  Fix: chmod 755 {metrics_file.parent}", err=True)
            except OSError as e:
                import logging

                logger = logging.getLogger(__name__)
                logger.error(f"Error writing metrics: {e}")
                click.echo(f"Warning: Cannot save metrics: {e}", err=True)
                click.echo("  Check disk space and filesystem permissions.", err=True)
            # KeyboardInterrupt, MemoryError, and other exceptions propagate naturally

            if show_metrics:
                click.echo("\n" + "=" * 60)
                click.echo(metrics.report(detailed=True))
                click.echo("=" * 60)

    finally:
        # Cleanup sensitive files
        if temp_dir and Path(temp_dir).exists():
            import logging
            import shutil

            logger = logging.getLogger(__name__)

            try:
                shutil.rmtree(temp_dir)
                logger.debug(f"Cleaned up temporary directory: {temp_dir}")
            except Exception as e:
                # SECURITY-CRITICAL: Keep broad catch for temp dir cleanup
                # If credentials aren't deleted, warn user but don't crash
                logger.error(
                    f"SECURITY: Failed to delete temporary directory "
                    f"containing API token: {temp_dir}",
                    extra={"error": str(e), "temp_dir": temp_dir},
                )
                click.echo(
                    f"SECURITY WARNING: Failed to clean up credentials at {temp_dir}",
                    err=True,
                )
                click.echo(f"  Manually delete: rm -rf {temp_dir}", err=True)

        # Close resources (ensure both are attempted even if one fails)
        try:
            await scanner.close()
        except OSError as e:
            import logging

            logger = logging.getLogger(__name__)
            logger.error(f"Error closing database connection: {e}")
            click.echo(f"Warning: Database cleanup failed: {e}", err=True)
        except Exception as e:
            import logging

            logger = logging.getLogger(__name__)
            logger.error(f"Unexpected error closing scanner: {e}", exc_info=True)
            # Re-raise unexpected errors for debugging
            raise

        try:
            await adapter.close()
        except OSError as e:
            import logging

            logger = logging.getLogger(__name__)
            logger.error(f"Error closing HTTP adapter: {e}")
            click.echo(f"Warning: HTTP adapter cleanup failed: {e}", err=True)
        except Exception as e:
            import logging

            logger = logging.getLogger(__name__)
            logger.error(f"Unexpected error closing adapter: {e}", exc_info=True)
            raise


@cli.command()
@click.argument("repository")
@click.argument("pr_number", type=int)
@click.option("--config", default=None, help="Config file path (optional, auto-discovers)")
@click.option("--post/--no-post", default=True, help="Post comments to PR (default: yes)")
def review(repository, pr_number, config, post):
    """Review a pull request with LLM analysis.

    Examples:
        drep review steve/drep 42
        drep review steve/drep 42 --no-post  # Dry run
    """
    if "/" not in repository:
        click.echo("Error: Repository must be in format 'owner/repo'", err=True)
        return

    owner, repo_name = repository.split("/", 1)

    # Discover config file
    config_path = find_config_file(config)
    if not config_path.exists():
        click.echo(f"Config file not found: {config_path}", err=True)
        click.echo("Run 'drep init' to create a config file.", err=True)
        return

    click.echo(f"Reviewing PR #{pr_number} in {owner}/{repo_name}...")

    try:
        # Run async review
        asyncio.run(_run_review(owner, repo_name, pr_number, str(config_path), post))
        click.echo("✓ Review complete")
    except FileNotFoundError:
        click.echo(f"Config file not found: {config_path}", err=True)
        click.echo("Run 'drep init' to create a config file.", err=True)
    except Exception as e:
        click.echo(f"Error during review: {e}", err=True)


async def _run_review(
    owner: str,
    repo: str,
    pr_number: int,
    config_path: str,
    post_comments: bool,
):
    """Run the PR review workflow.

    Args:
        owner: Repository owner
        repo: Repository name
        pr_number: PR number to review
        config_path: Path to config file
        post_comments: Whether to post comments to PR
    """
    # Load config
    config = load_config(config_path)

    # Check LLM is enabled
    if not config.llm or not config.llm.enabled:
        click.echo("Error: LLM must be enabled in config for PR reviews", err=True)
        return

    # Determine which adapter to use (prefer Gitea for backward compatibility)
    platform = None
    adapter = None

    if config.gitea is not None:
        # Use Gitea adapter
        platform = "gitea"
        adapter = GiteaAdapter(config.gitea.url, config.gitea.token.get_secret_value())
    elif config.github is not None:
        # Use GitHub adapter
        platform = "github"
        from drep.adapters.github import GitHubAdapter

        adapter = GitHubAdapter(
            token=config.github.token.get_secret_value(),
            url=str(config.github.url) if config.github.url else "https://api.github.com",
        )
    elif config.gitlab is not None:
        # Use GitLab adapter
        platform = "gitlab"
        from drep.adapters.gitlab import GitLabAdapter

        adapter = GitLabAdapter(
            token=config.gitlab.token.get_secret_value(),
            url=config.gitlab.url,  # None for GitLab.com, or custom URL
        )
    else:
        # No platform configured (shouldn't happen - Config validator requires at least one)
        click.echo(
            "Error: No platform configured. "
            "Please add [gitea], [github], or [gitlab] to your config.yaml.",
            err=True,
        )
        raise click.Abort()

    # Initialize components
    scanner = RepositoryScanner(init_database(config.database_url), config, gitea_adapter=adapter)

    try:
        # Check PR analyzer is available
        if not scanner.pr_analyzer:
            click.echo("Error: PR analyzer not initialized (LLM required)", err=True)
            return

        # Review PR
        click.echo(f"Fetching {platform} PR #{pr_number}...")
        result = await scanner.pr_analyzer.review_pr(owner, repo, pr_number)

        # Display results
        click.echo("\n=== Review Summary ===")
        click.echo(result.summary)
        click.echo(f"\nFound {len(result.comments)} comments")
        click.echo(f"Recommendation: {'✅ Approve' if result.approve else '🔍 Request Changes'}")

        if result.concerns:
            click.echo("\nConcerns:")
            for concern in result.concerns:
                click.echo(f"  - {concern}")

        # Show comments summary
        if result.comments:
            click.echo("\nComment breakdown:")
            severity_counts = {}
            for comment in result.comments:
                severity_counts[comment.severity] = severity_counts.get(comment.severity, 0) + 1
            for severity, count in sorted(severity_counts.items()):
                click.echo(f"  {severity}: {count}")

        # Post to PR (if enabled)
        if post_comments:
            click.echo("\nPosting review to PR...")
            pr_data = await adapter.get_pr(owner, repo, pr_number)
            commit_sha = pr_data["head"]["sha"]
            await scanner.pr_analyzer.post_review(owner, repo, pr_number, commit_sha, result)
            click.echo("✓ Review posted!")
        else:
            click.echo("\n(Dry run - not posting to PR)")

    finally:
        # Cleanup
        # Persist metrics if available
        if scanner.llm_client:
            try:
                import logging
                from pathlib import Path as _Path

                from drep.llm.metrics import MetricsCollector

                metrics_file = _Path.home() / ".drep" / "metrics.json"
                collector = MetricsCollector(metrics_file)
                collector.current_session = scanner.llm_client.get_llm_metrics()
                await collector.save()
            except Exception as e:
                import logging

                logger = logging.getLogger(__name__)
                logger.warning(f"Failed to persist LLM metrics: {e}")
                # Metrics are best-effort in cleanup, don't crash

        # Close resources (ensure both are attempted even if one fails)
        try:
            await scanner.close()
        except Exception as e:
            import logging

            logger = logging.getLogger(__name__)
            logger.error(f"Error closing scanner: {e}")

        try:
            await adapter.close()
        except Exception as e:
            import logging

            logger = logging.getLogger(__name__)
            logger.error(f"Error closing adapter: {e}")


@cli.command()
@click.argument("path", default=".")
@click.option("--staged", is_flag=True, help="Only check git staged files")
@click.option("--config", default=None, help="Config file path (optional for local-only mode)")
@click.option("--exit-zero", is_flag=True, help="Always exit with 0 (don't block commits)")
@click.option(
    "--format",
    type=click.Choice([OutputFormat.TEXT.value, OutputFormat.JSON.value]),
    default=OutputFormat.TEXT.value,
    help="Output format",
)
def check(path, staged, config, exit_zero, format):
    """Check local files without platform API (pre-commit friendly).

    Examples:
        drep check                    # Check current directory
        drep check --staged           # Check only staged files
        drep check path/to/file.py    # Check specific file
        drep check --exit-zero        # Warn without blocking commits
    """
    import asyncio

    # Discover config file if not explicitly disabled
    # For check command, config is truly optional (can run without it)
    config_path = None
    if config is not None:
        # User explicitly provided a config path - must exist
        config_file = find_config_file(config)
        if not config_file.exists():
            click.echo(f"Config file not found: {config_file}", err=True)
            raise SystemExit(1)
        config_path = str(config_file)
    else:
        # Try to discover config, but don't fail if not found
        config_file = find_config_file(None)
        if config_file.exists():
            config_path = str(config_file)

    # Run async check
    findings = asyncio.run(_run_check(path, staged, config_path, format))

    # Print findings summary
    if findings:
        if exit_zero:
            # Warning mode - print to stdout
            click.echo(f"\n⚠ Found {len(findings)} issue(s) (warning mode)")
        else:
            # Error mode - print to stderr
            click.echo(f"\n✗ Found {len(findings)} issue(s)", err=True)

        # Exit with appropriate code
        if not exit_zero:
            raise SystemExit(1)
    else:
        click.echo("✓ No issues found")


async def _run_check(path: str, staged: bool, config_path: str, output_format: str):
    """Run local file check without platform API.

    Args:
        path: Path to check (file or directory)
        staged: Only check staged files
        config_path: Config file path (optional)
        output_format: Output format (text or json)

    Returns:
        List of Finding objects
    """
    from pathlib import Path as PathLib

    from drep.config import load_config
    from drep.core.scanner import RepositoryScanner
    from drep.db import init_database

    # Load config with platform not required (pre-commit mode)
    if config_path:
        try:
            config = load_config(config_path, require_platform=False)
        except FileNotFoundError:
            click.echo(f"Error: Config file not found: {config_path}", err=True)
            raise SystemExit(1)
        except yaml.YAMLError as e:
            click.echo(f"Error: Invalid YAML in {config_path}\n{e}", err=True)
            raise SystemExit(1)
        except ValidationError as e:
            click.echo(f"Error: Configuration validation failed\n{e}", err=True)
            raise SystemExit(1)
        # DO NOT CATCH: KeyboardInterrupt, SystemExit, ImportError
        # These should propagate to allow proper termination and debugging
    else:
        # Create minimal config for local-only mode
        from drep.models.config import Config

        config = Config(require_platform_config=False)

    # Initialize database (in-memory for check command)
    db = init_database("sqlite:///:memory:")

    # Initialize scanner
    scanner = RepositoryScanner(db, config=config)

    try:
        # Validate and resolve path
        try:
            path_obj = PathLib(path).resolve(strict=True)
        except FileNotFoundError:
            click.echo(f"Error: Path not found: {path}", err=True)
            raise SystemExit(1)

        # Additional validation
        if not path_obj.exists():
            click.echo(f"Error: Path does not exist: {path}", err=True)
            raise SystemExit(1)

        if staged:
            # Get staged files from git index
            files = scanner.get_staged_files(str(path_obj))
            click.echo(f"Checking {len(files)} staged file(s)...")
        else:
            # Get all Python/Markdown files
            if path_obj.is_file():
                # Single file
                files = [str(path_obj.relative_to(path_obj.parent))]
            else:
                # Directory - get all .py and .md files
                files = scanner._get_all_python_files(str(path_obj))
            click.echo(f"Checking {len(files)} file(s)...")

        if not files:
            return []

        # Analyze files
        all_findings = []

        # Code quality analysis (if LLM enabled)
        if config.llm and config.llm.enabled:
            code_findings = await scanner.analyze_code_quality(
                repo_path=str(path_obj.parent if path_obj.is_file() else path_obj),
                files=files,
                repo_id="local",
                commit_sha="local",
            )
            all_findings.extend(code_findings)

        # Docstring analysis (if LLM enabled)
        if config.llm and config.llm.enabled:
            docstring_findings = await scanner.analyze_docstrings(
                repo_path=str(path_obj.parent if path_obj.is_file() else path_obj),
                files=files,
                repo_id="local",
                commit_sha="local",
            )
            all_findings.extend(docstring_findings)

        # Output findings
        if all_findings:
            _output_findings(all_findings, output_format)

        return all_findings

    finally:
        # Cleanup
        await scanner.close()


def _output_findings(findings, format_type):
    """Output findings in specified format.

    Args:
        findings: List of Finding objects
        format_type: OutputFormat value ('text' or 'json')
    """
    if format_type == OutputFormat.JSON.value:
        import json

        findings_dict = [f.model_dump() for f in findings]
        click.echo(json.dumps(findings_dict, indent=2))
    else:
        # Text format: file:line:column: severity: message
        click.echo()
        for finding in findings:
            col = f":{finding.column}" if finding.column else ""
            click.echo(
                f"{finding.file_path}:{finding.line}{col}: "
                f"{finding.severity}: {finding.message}"
            )
            if finding.suggestion:
                click.echo(f"  → {finding.suggestion}")


@cli.command()
@click.option("--config", default=None, help="Config file path (optional, auto-discovers)")
def validate(config):
    """Validate configuration file and environment variables.

    Loads the config in strict mode (env var placeholders must be set).
    """
    # Discover config file
    config_path = find_config_file(config)
    if not config_path.exists():
        click.echo(f"Config file not found: {config_path}", err=True)
        click.echo("Run 'drep init' to create a config file.", err=True)
        return

    try:
        _ = load_config(str(config_path), strict=True)
        click.echo(f"✓ Config valid: {config_path}")
    except Exception as e:
        click.echo(f"Invalid config: {e}", err=True)


@cli.command()
@click.option("--host", default="0.0.0.0", help="Host to bind")
@click.option("--port", default=8000, type=int, help="Port to listen on")
def serve(host, port):
    """Start the FastAPI server for webhooks and health checks."""
    try:
        import uvicorn

        uvicorn.run("drep.server:app", host=host, port=port, reload=False)
    except Exception as e:
        click.echo(f"Failed to start server: {e}", err=True)


@cli.command()
@click.option("--days", default=30, help="Days of history to show")
@click.option("--export", type=click.Path(), help="Export metrics to JSON file")
@click.option("--detailed/--summary", default=False, help="Show detailed breakdown")
def metrics(days, export, detailed):
    """Display LLM usage metrics and cost estimation.

    Examples:
        drep metrics --days 7
        drep metrics --detailed
        drep metrics --export metrics.json
    """
    import json
    from pathlib import Path

    from drep.llm.metrics import MetricsCollector

    # Load metrics
    metrics_file = Path.home() / ".drep" / "metrics.json"

    if not metrics_file.exists():
        click.echo("No metrics found. Run 'drep scan' first to generate metrics.")
        return

    collector = MetricsCollector(metrics_file)

    # Get aggregated metrics
    aggregated = collector.aggregate_history(days=days)

    # Display report
    click.echo(aggregated.report(detailed=detailed))

    # Export if requested
    if export:
        export_path = Path(export)
        with open(export_path, "w") as f:
            json.dump(aggregated.to_dict(), f, indent=2)
        click.echo(f"\n✓ Metrics exported to {export_path}")


@cli.command()
@click.argument("path", type=click.Path(exists=True))
@click.option("--output", type=click.Choice(["text", "json"]), default="text", help="Output format")
def lint_docs(path, output):
    """Lint markdown documentation files for style and formatting issues.

    Examples:
        drep lint-docs docs/
        drep lint-docs README.md
        drep lint-docs docs/ --output json
    """
    from pathlib import Path

    from drep.models.config import DocumentationConfig

    # Create analyzer with markdown checks enabled
    config = DocumentationConfig(enabled=True, markdown_checks=True)
    analyzer = DocumentationAnalyzer(config)

    # Find markdown files
    path_obj = Path(path)
    if path_obj.is_file():
        md_files = [path_obj] if path_obj.suffix == ".md" else []
    else:
        md_files = list(path_obj.rglob("*.md"))

    if not md_files:
        click.echo("No markdown files found.")
        return

    # Analyze all files
    total_issues = 0
    results = []

    for md_file in sorted(md_files):
        try:
            content = md_file.read_text(encoding="utf-8")
            findings = asyncio.run(analyzer.analyze_file(str(md_file), content))

            if findings.pattern_issues:
                total_issues += len(findings.pattern_issues)
                results.append((md_file, findings))
        except (IOError, OSError, UnicodeDecodeError) as e:
            click.echo(f"Error reading {md_file}: {e}", err=True)
        except Exception as e:
            # Unexpected error - show details and re-raise for debugging
            click.echo(f"Unexpected error analyzing {md_file}: {e}", err=True)
            raise

    # Output results
    if output == "json":
        import json

        json_output = []
        for md_file, findings in results:
            for issue in findings.pattern_issues:
                json_output.append(
                    {
                        "file": str(md_file),
                        "line": issue.line,
                        "column": issue.column,
                        "type": issue.type,
                        "message": issue.matched_text[:50],
                    }
                )
        click.echo(json.dumps(json_output, indent=2))
    else:
        # Text output
        if total_issues == 0:
            click.echo(f"✓ No issues found in {len(md_files)} markdown files.")
        else:
            click.echo(f"Found {total_issues} issues in {len(results)} files:\n")
            for md_file, findings in results:
                click.echo(f"{md_file}:")
                for issue in findings.pattern_issues:
                    click.echo(f"  Line {issue.line}:{issue.column} [{issue.type}]")
                click.echo()


if __name__ == "__main__":
    cli()
