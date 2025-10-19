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
  url: http://192.168.1.14:3000
  token: ${GITEA_TOKEN}
  repositories:
    - steve/*

documentation:
  enabled: true
  custom_dictionary:
    - asyncio
    - fastapi
    - gitea

database_url: sqlite:///./drep.db
"""

    config_path.write_text(example)
    click.echo("✓ Created config.yaml")
    click.echo("\nEdit config.yaml to add your Gitea token.")
    click.echo("Set GITEA_TOKEN environment variable before running scans.")


@cli.command()
@click.argument("repository")
@click.option("--config", default="config.yaml", help="Config file path")
def scan(repository, config):
    """Scan a repository: drep scan owner/repo"""

    if "/" not in repository:
        click.echo("Error: Repository must be in format 'owner/repo'", err=True)
        return

    owner, repo_name = repository.split("/", 1)

    click.echo(f"Scanning {owner}/{repo_name}...")

    try:
        # Run async scan
        asyncio.run(_run_scan(owner, repo_name, config))
        click.echo("✓ Scan complete")
    except FileNotFoundError:
        click.echo(f"Config file not found: {config}", err=True)
        click.echo("Run 'drep init' to create a config file.", err=True)
    except Exception as e:
        click.echo(f"Error during scan: {e}", err=True)


async def _run_scan(owner: str, repo: str, config_path: str):
    """Run the actual scan workflow.

    Args:
        owner: Repository owner
        repo: Repository name
        config_path: Path to config file
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
        askpass_script.chmod(0o755)

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
            click.echo("Running LLM-powered code quality analysis...")
            repo_id = f"{owner}/{repo}"
            code_findings = await scanner.analyze_code_quality(
                repo_path=str(repo_path),
                files=files,
                repo_id=repo_id,
                commit_sha=current_sha,
            )
            findings.extend(code_findings)

            # 3. Docstring analysis (LLM-powered)
            click.echo("Analyzing docstrings...")
            docstring_findings = await scanner.analyze_docstrings(
                repo_path=str(repo_path),
                files=files,
                repo_id=repo_id,
                commit_sha=current_sha,
            )
            findings.extend(docstring_findings)

        click.echo(f"Found {len(findings)} issues")

        # Create issues
        if findings:
            await issue_manager.create_issues_for_findings(owner, repo, findings)

        # Record scan
        scanner.record_scan(owner, repo, current_sha)

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
        await scanner.close()
        await adapter.close()


if __name__ == "__main__":
    cli()
