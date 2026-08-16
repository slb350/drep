"""CLI interface for drep."""

import asyncio
import json
import os
import shutil
from enum import Enum
from pathlib import Path

import click
import yaml
from pydantic_core import ValidationError

from drep.cli_wizard import (
    _collect_database_config,
    _collect_documentation_config,
    _collect_llm_config,
    _collect_platform_config,
    _write_and_validate_config,
)
from drep.cli_workflows import _run_review, _run_scan
from drep.config import find_config_file, get_user_config_dir, load_config
from drep.constants import default_metrics_file
from drep.core.scanner import RepositoryScanner
from drep.db import init_database
from drep.documentation.analyzer import DocumentationAnalyzer
from drep.models.config import Config


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
        asyncio.run(_run_review(owner, repo_name, pr_number, str(config_path), post))
        click.echo("✓ Review complete")
    except FileNotFoundError:
        click.echo(f"Config file not found: {config_path}", err=True)
        click.echo("Run 'drep init' to create a config file.", err=True)
    except Exception as e:
        click.echo(f"Error during review: {e}", err=True)


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

    # Load config with platform not required (pre-commit mode)
    if config_path:
        try:
            config = load_config(config_path, require_platform=False)
        except FileNotFoundError as exc:
            click.echo(f"Error: Config file not found: {config_path}", err=True)
            raise SystemExit(1) from exc
        except yaml.YAMLError as e:
            click.echo(f"Error: Invalid YAML in {config_path}\n{e}", err=True)
            raise SystemExit(1) from e
        except ValidationError as e:
            click.echo(f"Error: Configuration validation failed\n{e}", err=True)
            raise SystemExit(1) from e
        # DO NOT CATCH: KeyboardInterrupt, SystemExit, ImportError
        # These should propagate to allow proper termination and debugging
    else:
        # Create minimal config for local-only mode

        config = Config(require_platform_config=False)

    # Initialize database (in-memory for check command)
    db = init_database("sqlite:///:memory:")

    # Initialize scanner
    scanner = RepositoryScanner(db, config=config)

    try:
        # Validate and resolve path
        try:
            path_obj = PathLib(path).resolve(strict=True)
        except FileNotFoundError as exc:
            click.echo(f"Error: Path not found: {path}", err=True)
            raise SystemExit(1) from exc

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
                files = scanner.get_scan_targets(str(path_obj))
            click.echo(f"Checking {len(files)} file(s)...")

        if not files:
            return []

        # Analyze files
        all_findings = []

        if config.llm and config.llm.enabled:
            analysis_root = str(path_obj.parent if path_obj.is_file() else path_obj)
            # The two passes are independent; run them concurrently. The rate
            # limiter provides the back-pressure.
            code_findings, docstring_findings = await asyncio.gather(
                scanner.analyze_code_quality(
                    repo_path=analysis_root,
                    files=files,
                    repo_id="local",
                    commit_sha="local",
                ),
                scanner.analyze_docstrings(
                    repo_path=analysis_root,
                    files=files,
                    repo_id="local",
                    commit_sha="local",
                ),
            )
            all_findings.extend(code_findings)
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
        findings_dict = [f.model_dump() for f in findings]
        click.echo(json.dumps(findings_dict, indent=2))
    else:
        # Text format: file:line:column: severity: message
        click.echo()
        for finding in findings:
            col = f":{finding.column}" if finding.column else ""
            click.echo(
                f"{finding.file_path}:{finding.line}{col}: {finding.severity}: {finding.message}"
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
        except (  # noqa: PERF203 - per-file isolation: one bad file must not abort the scan
            OSError,
            UnicodeDecodeError,
        ) as e:
            click.echo(f"Error reading {md_file}: {e}", err=True)
        except Exception as e:
            # Unexpected error - show details and re-raise for debugging
            click.echo(f"Unexpected error analyzing {md_file}: {e}", err=True)
            raise

    # Output results
    if output == "json":
        json_output = [
            {
                "file": str(md_file),
                "line": issue.line,
                "column": issue.column,
                "type": issue.type,
                "message": issue.matched_text[:50],
            }
            for md_file, findings in results
            for issue in findings.pattern_issues
        ]
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
