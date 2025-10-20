"""CLI interface for drep."""

import asyncio
import os
import tempfile
from pathlib import Path

import click
from git import Repo

from drep.adapters.gitea import GiteaAdapter
from drep.config import load_config
from drep.core.issue_manager import IssueManager
from drep.core.scanner import RepositoryScanner
from drep.db import init_database
from drep.documentation.analyzer import DocumentationAnalyzer


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

    # Create example config
    example = """gitea:
  url: http://localhost:3000
  token: ${GITEA_TOKEN}
  repositories:
    - your-org/*

documentation:
  enabled: true
  custom_dictionary: []

database_url: sqlite:///./drep.db

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

    config_path.write_text(example)
    click.echo("✓ Created config.yaml")
    click.echo("\nEdit config.yaml to add your Gitea token.")
    click.echo("Set GITEA_TOKEN environment variable before running scans.")


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

    # Initialize components
    adapter = GiteaAdapter(config.gitea.url, config.gitea.token)
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

        # Create askpass script
        askpass_content = """#!/bin/sh
if echo "$1" | grep -qi "username"; then
    echo "token"
elif echo "$1" | grep -qi "password"; then
    echo "$DREP_GIT_TOKEN"
else
    echo "$DREP_GIT_TOKEN"
fi
"""
        askpass_script.write_text(askpass_content)
        # Restrict to owner only; contains sensitive token usage
        askpass_script.chmod(0o700)

        # Build git environment
        git_env = {
            **os.environ,
            "GIT_ASKPASS": str(askpass_script),
            "GIT_TERMINAL_PROMPT": "0",
            "DREP_GIT_TOKEN": config.gitea.token,
        }

        # Repository path
        repo_path = Path("./repos") / owner / repo

        # Clone or pull repository
        if not repo_path.exists():
            click.echo("Cloning repository...")
            repo_path.parent.mkdir(parents=True, exist_ok=True)

            # Get default branch
            default_branch = await adapter.get_default_branch(owner, repo)

            # Clone
            clean_git_url = f"{config.gitea.url.rstrip('/')}/{owner}/{repo}.git"
            Repo.clone_from(clean_git_url, repo_path, branch=default_branch, env=git_env)
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
                content = full_path.read_text(errors="ignore")
                result = await analyzer.analyze_file(file_path, content)
                findings.extend(result.to_findings())

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
                from drep.llm.metrics import MetricsCollector
                from pathlib import Path as _Path

                metrics_file = _Path.home() / ".drep" / "metrics.json"
                collector = MetricsCollector(metrics_file)
                collector.current_session = metrics
                await collector.save()
            except Exception as e:
                click.echo(f"Warning: failed to persist metrics: {e}")

            if show_metrics:
                click.echo("\n" + "=" * 60)
                click.echo(metrics.report(detailed=True))
                click.echo("=" * 60)

    finally:
        # Cleanup
        if temp_dir and Path(temp_dir).exists():
            import shutil

            shutil.rmtree(temp_dir, ignore_errors=True)

        # Close resources
        await scanner.close()
        await adapter.close()


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

    # Initialize components
    adapter = GiteaAdapter(config.gitea.url, config.gitea.token)
    scanner = RepositoryScanner(init_database(config.database_url), config, gitea_adapter=adapter)

    try:
        # Check PR analyzer is available
        if not scanner.pr_analyzer:
            click.echo("Error: PR analyzer not initialized (LLM required)", err=True)
            return

        # Review PR
        click.echo(f"Fetching PR #{pr_number}...")
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
                from drep.llm.metrics import MetricsCollector
                from pathlib import Path as _Path

                metrics_file = _Path.home() / ".drep" / "metrics.json"
                collector = MetricsCollector(metrics_file)
                collector.current_session = scanner.llm_client.get_llm_metrics()
                await collector.save()
            except Exception:
                pass
        await scanner.close()
        await adapter.close()


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


if __name__ == "__main__":
    cli()
