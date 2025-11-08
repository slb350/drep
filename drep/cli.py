"""CLI interface for drep."""

import asyncio
import os
import tempfile
from enum import Enum
from pathlib import Path

import click
import yaml
from git import Repo
from pydantic import ValidationError

from drep.adapters.gitea import GiteaAdapter
from drep.config import load_config
from drep.core.issue_manager import IssueManager
from drep.core.scanner import RepositoryScanner
from drep.db import init_database
from drep.documentation.analyzer import DocumentationAnalyzer


class OutputFormat(str, Enum):
    """Output format options for check command."""

    TEXT = "text"
    JSON = "json"


@click.group()
def cli():
    """drep - Documentation & Review Enhancement Platform"""
    pass


@cli.command()
def init():
    """Initialize drep configuration."""
    config_path = Path("config.yaml")

    if config_path.exists():
        click.confirm("config.yaml already exists. Overwrite?", abort=True)

    # Ask user which platform they're using
    click.echo("Which git platform are you using?")
    platform = click.prompt(
        "Choose platform",
        type=click.Choice(["github", "gitea", "gitlab"], case_sensitive=False),
        default="github",
    )

    # Common LLM config (used by all platforms)
    llm_config = """
llm:
  enabled: true
  endpoint: http://localhost:1234/v1  # LM Studio / Ollama (with OpenAI compatible API)
  model: qwen3-30b-a3b
  temperature: 0.2
  max_tokens: 8000
  timeout: 120
  max_retries: 3
  retry_delay: 2
  max_concurrent_global: 5
  max_concurrent_per_repo: 3
  requests_per_minute: 60
  max_tokens_per_minute: 80000
  cache:
    enabled: true
    ttl_days: 30
"""

    # Platform-specific configs
    if platform.lower() == "github":
        platform_config = """github:
  token: ${GITHUB_TOKEN}
  # url: https://api.github.com  # Optional: for GitHub Enterprise, use https://your-domain/api/v3
  repositories:
    - your-org/*

documentation:
  enabled: true
  custom_dictionary: []

database_url: sqlite:///./drep.db
"""
        example = platform_config + llm_config
        env_var = "GITHUB_TOKEN"
        platform_name = "GitHub"

    elif platform.lower() == "gitea":
        platform_config = """gitea:
  url: http://localhost:3000
  token: ${GITEA_TOKEN}
  repositories:
    - your-org/*

documentation:
  enabled: true
  custom_dictionary: []

database_url: sqlite:///./drep.db
"""
        example = platform_config + llm_config
        env_var = "GITEA_TOKEN"
        platform_name = "Gitea"

    else:  # gitlab
        platform_config = """gitlab:
  url: https://gitlab.com
  token: ${GITLAB_TOKEN}
  repositories:
    - your-org/*

documentation:
  enabled: true
  custom_dictionary: []

database_url: sqlite:///./drep.db
"""
        example = platform_config + llm_config
        env_var = "GITLAB_TOKEN"
        platform_name = "GitLab"

    config_path.write_text(example)
    click.echo(f"✓ Created config.yaml for {platform_name}")
    click.echo("\nNext steps:")
    click.echo(f"1. Edit config.yaml to configure your {platform_name} URL (if needed)")
    click.echo(f"2. Set {env_var} environment variable with your API token")
    click.echo("3. Update the repositories list to match your org/repos")
    click.echo("\nThen run: drep scan owner/repo")


@cli.command()
@click.argument("repository")
@click.option("--config", default="config.yaml", help="Config file path")
@click.option("--show-metrics/--no-metrics", default=False, help="Show LLM metrics after scan")
@click.option("--show-progress/--no-progress", default=True, help="Show progress during scan")
def scan(repository, config, show_metrics, show_progress):
    """Scan a repository: drep scan owner/repo"""

    if "/" not in repository:
        click.echo("Error: Repository must be in format 'owner/repo'", err=True)
        return

    owner, repo_name = repository.split("/", 1)

    click.echo(f"Scanning {owner}/{repo_name}...")

    try:
        # Run async scan
        asyncio.run(_run_scan(owner, repo_name, config, show_metrics, show_progress))
        click.echo("✓ Scan complete")
    except click.Abort:
        # Re-raise to let Click handle the abort (already displayed error message)
        raise
    except FileNotFoundError:
        click.echo(f"Config file not found: {config}", err=True)
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

        # Write token to temporary file with owner-only read permissions
        # This prevents token exposure in process environment variables
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
        if not repo_path.exists():
            click.echo(f"Cloning {platform} repository...")
            repo_path.parent.mkdir(parents=True, exist_ok=True)

            # Get default branch
            default_branch = await adapter.get_default_branch(owner, repo)

            # Clone
            Repo.clone_from(git_url, repo_path, branch=default_branch, env=git_env)
        else:
            click.echo("Pulling latest changes...")
            git_repo = Repo(repo_path)
            with git_repo.git.custom_environment(**git_env):
                git_repo.remotes.origin.pull()

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
                collector = MetricsCollector(metrics_file)
                collector.current_session = metrics
                await collector.save()
            except Exception as e:
                import logging

                logger = logging.getLogger(__name__)
                logger.warning(f"Failed to persist LLM metrics: {e}")
                click.echo(f"Warning: failed to persist metrics: {e}")

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
                logger.error(
                    f"SECURITY: Failed to delete temporary directory "
                    f"containing API token: {temp_dir}",
                    extra={"error": str(e), "temp_dir": temp_dir},
                )
                click.echo(
                    f"WARNING: Failed to clean up temporary credentials at {temp_dir}. "
                    f"Please manually delete this directory: {e}",
                    err=True,
                )

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
@click.argument("repository")
@click.argument("pr_number", type=int)
@click.option("--config", default="config.yaml", help="Config file path")
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

    click.echo(f"Reviewing PR #{pr_number} in {owner}/{repo_name}...")

    try:
        # Run async review
        asyncio.run(_run_review(owner, repo_name, pr_number, config, post))
        click.echo("✓ Review complete")
    except FileNotFoundError:
        click.echo(f"Config file not found: {config}", err=True)
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

    # Run async check
    findings = asyncio.run(_run_check(path, staged, config, format))

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
@click.option("--config", default="config.yaml", help="Config file path")
def validate(config):
    """Validate configuration file and environment variables.

    Loads the config in strict mode (env var placeholders must be set).
    """
    try:
        _ = load_config(config, strict=True)
        click.echo(f"✓ Config valid: {config}")
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
