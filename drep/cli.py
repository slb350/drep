"""CLI interface for drep."""

import asyncio
import json
import os
import shutil
from enum import Enum
from pathlib import Path

import click

from drep.cli_wizard import (
    _collect_database_config,
    _collect_documentation_config,
    _collect_llm_config,
    _collect_platform_config,
    _write_and_validate_config,
)
from drep.config import find_config_file, get_user_config_dir, load_config
from drep.constants import default_metrics_file
from drep.models.findings import SEVERITY_RANK


class OutputFormat(str, Enum):
    """Output format options for check command."""

    TEXT = "text"
    JSON = "json"


def _resolve_config_file(config: str | None) -> Path | None:
    """Discover the config file, reporting the standard error if it is missing.

    Returns:
        The config path, or None if it does not exist (the message has already
        been echoed, so the caller should just return).
    """
    config_path = find_config_file(config)
    if not config_path.exists():
        click.echo(f"Config file not found: {config_path}", err=True)
        click.echo("Run 'drep init' to create a config file.", err=True)
        return None
    return config_path


@click.group()
def cli():
    """drep - Documentation & Review Enhancement Platform"""


@cli.command()
def init():
    """Initialize drep configuration with interactive setup wizard.

    Guides the user through a multi-step wizard to configure:
    1. Configuration location (current directory or user config directory)
    2. Platform selection (GitHub/Gitea/GitLab) with platform-specific options
    3. LLM configuration (optional) - supports OpenAI-compatible and Bedrock
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
        except PermissionError as exc:
            click.echo(f"ERROR: Cannot create directory {config_path.parent}", err=True)
            click.echo("  Permission denied. Try using location 1 (current directory).", err=True)
            raise click.Abort() from exc
        except OSError as e:
            click.echo(f"ERROR: Cannot create directory: {e}", err=True)
            raise click.Abort() from e

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
            except PermissionError as exc:
                click.echo(
                    f"ERROR: Cannot create backup at {backup_path}\n"
                    f"Permission denied. Cannot safely overwrite config.",
                    err=True,
                )
                raise click.Abort() from exc
            except OSError as e:
                click.echo(
                    f"ERROR: Cannot create backup: {e}\n"
                    f"Cannot safely overwrite config without backup.",
                    err=True,
                )
                raise click.Abort() from e
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

    # The data variants know which variables they need; the command layer does
    # not re-derive it from provider strings or the serialized YAML.
    llm_env_vars = llm_config.data.required_env_vars() if llm_config else ()
    for var in llm_env_vars:
        click.echo(f"   export {var}='...'")

    if click.confirm("\nCheck if required environment variables are set?", default=False):
        # SECURITY: Check environment variables with error handling
        # This code reads required tokens from os.environ to verify they're set before
        # the user runs a scan. It catches OSError and PermissionError (restricted
        # environments like containers/sandboxes may restrict os.environ access) and
        # displays a warning. KeyboardInterrupt propagates naturally to allow wizard abort.
        try:
            required = (platform_config.env_var, *llm_env_vars)
            missing = [var for var in required if var not in os.environ]

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
    config_path = _resolve_config_file(config)
    if config_path is None:
        return

    click.echo(f"Scanning {owner}/{repo_name}...")

    try:
        # Run async scan
        # Imported here, not at module scope: cli_workflows pulls in sqlalchemy,
        # GitPython and the LLM client, none of which `lint-docs` touches - and
        # lint-docs runs on every commit as a pre-commit hook.
        from drep.cli_workflows import _run_scan

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
    config_path = _resolve_config_file(config)
    if config_path is None:
        return

    click.echo(f"Reviewing PR #{pr_number} in {owner}/{repo_name}...")

    try:
        # Run async review
        from drep.cli_workflows import _run_review

        asyncio.run(_run_review(owner, repo_name, pr_number, str(config_path), post))
        click.echo("✓ Review complete")
    except FileNotFoundError:
        click.echo(f"Config file not found: {config_path}", err=True)
        click.echo("Run 'drep init' to create a config file.", err=True)
    except Exception as e:
        click.echo(f"Error during review: {e}", err=True)


@cli.command()
@click.argument("paths", nargs=-1, type=click.Path())
@click.option("--staged", is_flag=True, help="Only check git staged files")
@click.option("--config", default=None, help="Config file path (optional for local-only mode)")
@click.option("--exit-zero", is_flag=True, help="Always exit with 0 (don't block commits)")
@click.option(
    "--format",
    type=click.Choice([OutputFormat.TEXT.value, OutputFormat.JSON.value]),
    default=OutputFormat.TEXT.value,
    help="Output format",
)
@click.option(
    "--fail-on",
    type=click.Choice(list(SEVERITY_RANK)),
    default=None,
    help=(
        "Also block on LLM findings at or above this severity. "
        "Deterministic tool findings always block."
    ),
)
def check(paths, staged, config, exit_zero, format, fail_on):
    """Check local files without platform API (pre-commit friendly).

    Examples:
        drep check                    # Check current directory
        drep check --staged           # Check only staged files
        drep check a.py b.py src/     # Check specific files/directories
        drep check --exit-zero        # Warn without blocking commits
        drep check --fail-on error    # Only bugs block; style notes just report

    Exit codes:
        0  analysis ran and found nothing above --fail-on (or --exit-zero)
        1  analysis ran and found issues at or above --fail-on
        2  one or more files could not be analyzed - the result is not a pass
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
    from drep.cli_workflows import _run_check

    outcome = asyncio.run(_run_check(paths or (".",), staged, config_path))
    findings = outcome.blocking + outcome.findings

    # Deterministic tool findings always block: they come from the rules the
    # project itself configured, so they are precise enough to gate on.
    blocking = list(outcome.blocking)

    # LLM findings only block if the user opts in with --fail-on. The model
    # reports style suggestions on nearly every file, so gating on them by
    # default means nothing ever passes.
    if fail_on is not None:
        # Indexing rather than .get(): Finding.severity is a validated
        # Severity, so an unrankable value is a bug to surface, not a finding
        # to silently pass.
        threshold = SEVERITY_RANK[fail_on]
        blocking += [f for f in outcome.findings if SEVERITY_RANK[f.severity] >= threshold]

    # JSON is a machine channel: always emit, so "no findings" and "nothing ran"
    # are distinguishable without parsing prose.
    if format == OutputFormat.JSON.value or findings:
        _output_findings(outcome, format)

    # In JSON mode stdout carries the payload and nothing else, so the human
    # summary goes to stderr rather than corrupting it.
    summary_to_stderr = format == OutputFormat.JSON.value

    if findings:
        advisory = len(findings) - len(blocking)
        if exit_zero:
            # Warning mode
            click.echo(f"\n⚠ Found {len(findings)} issue(s) (warning mode)", err=summary_to_stderr)
        elif blocking:
            # Error mode - print to stderr. Naming both halves matters: the
            # blocking count is what the user has to act on, the rest is
            # information they can weigh.
            summary = f"\n✗ {len(blocking)} blocking issue(s)"
            if advisory:
                summary += f", {advisory} advisory"
            click.echo(summary, err=True)
        else:
            click.echo(f"\n⚠ {advisory} advisory issue(s), none blocking", err=summary_to_stderr)
    elif not outcome.incomplete:
        click.echo("✓ No issues found", err=summary_to_stderr)

    # An unanalyzed file is not a clean file. Reported after the findings so it
    # is the last thing on screen, and with its own exit code so a hook can tell
    # "your code has issues" from "drep could not run".
    if outcome.unavailable_tools:
        click.echo(
            f"\n✗ {len(outcome.unavailable_tools)} tool(s) were configured but could not run, "
            f"so those checks did not happen: {', '.join(outcome.unavailable_tools)}",
            err=True,
        )

    if outcome.failed_files:
        click.echo(
            f"\n✗ {len(outcome.failed_files)} file(s) could not be analyzed - results are "
            f"incomplete (see errors above): {', '.join(outcome.failed_files)}",
            err=True,
        )

    if exit_zero:
        return
    if outcome.incomplete:
        raise SystemExit(2)
    if blocking:
        raise SystemExit(1)


def _echo_finding(finding, *, with_suggestion: bool) -> None:
    """One finding as `file:line:col: severity: [rule] message`.

    The rule code is the actionable half of a deterministic finding: without it
    you cannot look the rule up or suppress it.
    """
    col = f":{finding.column}" if finding.column else ""
    click.echo(
        f"{finding.file_path}:{finding.line}{col}: "
        f"{finding.severity}: [{finding.type}] {finding.message}"
    )
    if with_suggestion and finding.suggestion:
        click.echo(f"  → {finding.suggestion}")


def _output_findings(outcome, format_type):
    """Output an analysis outcome in the specified format.

    Takes the whole outcome, not just its findings: a bare findings array looks
    identical whether every file was analyzed or none were, leaving a JSON
    consumer to infer incompleteness from an exit code it may never see.

    Args:
        outcome: AnalysisResult from the run
        format_type: OutputFormat value ('text' or 'json')
    """
    if format_type == OutputFormat.JSON.value:
        click.echo(
            json.dumps(
                {
                    "blocking": [f.model_dump(mode="json") for f in outcome.blocking],
                    "findings": [f.model_dump(mode="json") for f in outcome.findings],
                    "unanalyzed": outcome.failed_files,
                    "unavailable_tools": outcome.unavailable_tools,
                },
                indent=2,
            )
        )
    else:
        # Text format: file:line:column: severity: [type] message
        #
        # Blocking findings print in full - they are what the user has to act
        # on. Advisory ones print one line each: 122 of them with their
        # multi-line suggestions overwhelmed pre-commit's writer and crashed
        # it. The full text stays available via --format json.
        click.echo()
        for finding in outcome.blocking:
            _echo_finding(finding, with_suggestion=True)
        for finding in outcome.findings:
            _echo_finding(finding, with_suggestion=False)


@cli.command()
@click.option("--config", default=None, help="Config file path (optional, auto-discovers)")
def validate(config):
    """Validate configuration file and environment variables.

    Loads the config in strict mode (env var placeholders must be set).
    """
    # Discover config file
    config_path = _resolve_config_file(config)
    if config_path is None:
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
    metrics_file = default_metrics_file()

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
        with export_path.open("w") as f:
            json.dump(aggregated.to_dict(), f, indent=2)
        click.echo(f"\n✓ Metrics exported to {export_path}")


# Registers the lint-docs command on the group above. Imported last, and for
# its side effect, because it imports `cli` from here.
from drep import cli_lint as _cli_lint  # noqa: E402,F401

if __name__ == "__main__":
    cli()
