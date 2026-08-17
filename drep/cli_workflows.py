"""Scan and review workflow implementations backing the CLI commands.

_run_scan clones/updates the repository, runs the analyzers, and files
issues; _run_review runs the LLM PR review pipeline. Both are also used
by the webhook server (drep.server).
"""

import asyncio
import logging
import os
import shutil
import tempfile
from dataclasses import dataclass
from pathlib import Path

import click
import yaml
from git import Repo
from git.exc import GitCommandError, InvalidGitRepositoryError
from pydantic_core import ValidationError

from drep.adapters.base import BaseAdapter
from drep.adapters.gitea import GiteaAdapter
from drep.adapters.github import GitHubAdapter
from drep.adapters.gitlab import GitLabAdapter
from drep.config import Config, load_config
from drep.constants import default_metrics_file
from drep.core.issue_manager import IssueManager
from drep.core.scanner import RepositoryScanner
from drep.db import init_database
from drep.documentation.analyzer import DocumentationAnalyzer
from drep.llm.metrics import LLMMetrics, MetricsCollector
from drep.models.findings import Finding
from drep.models.wizard import PLATFORM_TOKEN_ENV_VARS

logger = logging.getLogger(__name__)


def _resolve_platform(config: Config, owner: str, repo: str) -> tuple[str, BaseAdapter, str, str]:
    """Resolve the platform adapter, git clone URL, and token from config.

    Single source of truth for platform selection (precedence: gitea,
    then github, then gitlab — Gitea first for backward compatibility),
    shared by the scan and review workflows. The clone URL comes from the
    adapter, which already holds the platform's base URL.

    Args:
        config: Loaded configuration
        owner: Repository owner
        repo: Repository name

    Returns:
        Tuple of (platform name, adapter instance, git URL, git token)

    Raises:
        click.Abort: If no platform is configured
    """
    if config.gitea is not None:
        adapter: BaseAdapter = GiteaAdapter(config.gitea.url, config.gitea.token.get_secret_value())
        token = config.gitea.token.get_secret_value()
        return "gitea", adapter, adapter.git_clone_url(owner, repo), token

    if config.github is not None:
        adapter = GitHubAdapter(
            token=config.github.token.get_secret_value(),
            url=str(config.github.url) if config.github.url else "https://api.github.com",
        )
        token = config.github.token.get_secret_value()
        return "github", adapter, adapter.git_clone_url(owner, repo), token

    if config.gitlab is not None:
        adapter = GitLabAdapter(
            token=config.gitlab.token.get_secret_value(),
            url=config.gitlab.url,  # None for GitLab.com, or custom URL
        )
        token = config.gitlab.token.get_secret_value()
        return "gitlab", adapter, adapter.git_clone_url(owner, repo), token

    # No platform configured (shouldn't happen - Config validator requires at least one)
    click.echo(
        "Error: No platform configured. "
        "Please add [gitea], [github], or [gitlab] to your config.yaml.",
        err=True,
    )
    raise click.Abort()


async def _persist_metrics(metrics: LLMMetrics) -> None:
    """Append a session's LLM metrics to the shared history file.

    Best-effort: metrics are observability, never a reason to fail a scan or a
    review. Shared by both workflows so the path and the error handling cannot
    diverge between them.
    """
    metrics_file = default_metrics_file()
    try:
        metrics_file.parent.mkdir(parents=True, exist_ok=True)
        collector = MetricsCollector(metrics_file)
        collector.current_session = metrics
        await collector.save()
    except PermissionError:
        logger.error(f"Permission denied writing metrics to {metrics_file}")
        click.echo(f"Warning: Cannot save metrics to {metrics_file}", err=True)
        click.echo(f"  Fix: chmod 755 {metrics_file.parent}", err=True)
    except OSError as e:
        logger.error(f"Error writing metrics: {e}")
        click.echo(f"Warning: Cannot save metrics: {e}", err=True)
        click.echo("  Check disk space and filesystem permissions.", err=True)
    # KeyboardInterrupt, MemoryError, and other exceptions propagate naturally


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
    platform, adapter, git_url, git_token = _resolve_platform(config, owner, repo)

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
                    logger.error(f"Failed to get default branch for {owner}/{repo}: {e}")
                    click.echo(f"Error: Cannot access repository {owner}/{repo}", err=True)
                    click.echo("  Check that repository exists and token has access.", err=True)
                    raise click.Abort() from e

                if not default_branch:
                    click.echo(f"Error: Repository {owner}/{repo} has no branches", err=True)
                    raise click.Abort()

                # Clone
                try:
                    Repo.clone_from(git_url, repo_path, branch=default_branch, env=git_env)
                except GitCommandError as e:
                    error_msg = str(e).lower()
                    if "authentication failed" in error_msg:
                        # Suggest the token env var for this platform
                        token_env_var = PLATFORM_TOKEN_ENV_VARS[platform]
                        click.echo("Error: Authentication failed", err=True)
                        click.echo(f"  Check your {token_env_var} token is valid", err=True)
                    elif "not found" in error_msg:
                        click.echo(f"Error: Repository {owner}/{repo} not found", err=True)
                        click.echo("  Verify repository exists and token has access", err=True)
                    else:
                        click.echo(f"Error: Git clone failed: {e}", err=True)
                    raise click.Abort() from e
            else:
                click.echo("Pulling latest changes...")
                try:
                    git_repo = Repo(repo_path)
                    with git_repo.git.custom_environment(**git_env):
                        git_repo.remotes.origin.pull()
                except GitCommandError as e:
                    logger.error(f"Git pull failed for {owner}/{repo}: {e}")
                    click.echo(f"Error: Git pull failed: {e}", err=True)
                    click.echo(f"  Try: rm -rf {repo_path} to force re-clone", err=True)
                    raise click.Abort() from e
                except InvalidGitRepositoryError as exc:
                    logger.error(f"Corrupted git repository at {repo_path}")
                    click.echo(f"Error: Corrupted git repository at {repo_path}", err=True)
                    click.echo(f"  Fix: rm -rf {repo_path} and re-run scan", err=True)
                    raise click.Abort() from exc
        except PermissionError as e:
            logger.error(f"Permission denied accessing {repo_path}: {e}")
            click.echo(f"Error: Cannot write to {repo_path}", err=True)
            click.echo("  Check directory permissions", err=True)
            raise click.Abort() from e

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
        unanalyzed: set[str] = set()
        if config.llm and config.llm.enabled:
            click.echo("Analyzing code quality...")
            repo_id = f"{owner}/{repo}"
            code_result = await scanner.analyze_code_quality(
                repo_path=str(repo_path),
                files=files,
                repo_id=repo_id,
                commit_sha=current_sha,
                progress_callback=update_progress if show_progress else None,
            )

            if show_progress:
                click.echo("")  # New line after progress bar completes

            findings.extend(code_result.findings)
            unanalyzed.update(code_result.failed_files)

            # 3. Docstring analysis (LLM-powered)
            click.echo("Analyzing docstrings...")
            docstring_result = await scanner.analyze_docstrings(
                repo_path=str(repo_path),
                files=files,
                repo_id=repo_id,
                commit_sha=current_sha,
                progress_callback=update_progress if show_progress else None,
            )

            if show_progress:
                click.echo("")  # New line after progress bar completes

            findings.extend(docstring_result.findings)
            unanalyzed.update(docstring_result.failed_files)

        click.echo(f"Found {len(findings)} issues")

        # "Found N issues" over a partial scan reads as a clean bill of health
        # for files the LLM never saw. Say which ones were missed.
        if unanalyzed:
            click.echo(
                f"Warning: {len(unanalyzed)} file(s) could not be analyzed, "
                f"so this scan is incomplete: {', '.join(sorted(unanalyzed))}",
                err=True,
            )

        # Create issues
        if findings:
            await issue_manager.create_issues_for_findings(owner, repo, findings)

        # Record scan
        scanner.record_scan(owner, repo, current_sha)

        # Persist and/or show metrics at end
        if scanner.llm_client:
            metrics = scanner.llm_client.get_llm_metrics()

            await _persist_metrics(metrics)

            if show_metrics:
                click.echo("\n" + "=" * 60)
                click.echo(metrics.report(detailed=True))
                click.echo("=" * 60)

    finally:
        # Cleanup sensitive files
        if temp_dir and Path(temp_dir).exists():
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
            logger.error(f"Error closing database connection: {e}")
            click.echo(f"Warning: Database cleanup failed: {e}", err=True)
        except Exception as e:
            logger.error(f"Unexpected error closing scanner: {e}", exc_info=True)
            # Re-raise unexpected errors for debugging
            raise

        try:
            await adapter.close()
        except OSError as e:
            logger.error(f"Error closing HTTP adapter: {e}")
            click.echo(f"Warning: HTTP adapter cleanup failed: {e}", err=True)
        except Exception as e:
            logger.error(f"Unexpected error closing adapter: {e}", exc_info=True)
            raise


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
    platform, adapter, _git_url, _git_token = _resolve_platform(config, owner, repo)

    # Initialize components
    scanner = RepositoryScanner(init_database(config.database_url), config, adapter=adapter)

    try:
        # Check PR analyzer is available
        if not scanner.pr_analyzer:
            click.echo("Error: PR analyzer not initialized (LLM required)", err=True)
            return

        # Review PR
        click.echo(f"Fetching {platform} PR #{pr_number}...")
        prepared = await scanner.pr_analyzer.review_pr(owner, repo, pr_number)
        result = prepared.result

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
            severity_counts: dict[str, int] = {}
            for comment in result.comments:
                severity_counts[comment.severity] = severity_counts.get(comment.severity, 0) + 1
            for severity, count in sorted(severity_counts.items()):
                click.echo(f"  {severity}: {count}")

        # Post to PR (if enabled)
        if post_comments:
            click.echo("\nPosting review to PR...")
            await scanner.pr_analyzer.post_review(prepared)
            click.echo("✓ Review posted!")
        else:
            click.echo("\n(Dry run - not posting to PR)")

    finally:
        # Cleanup
        # Persist metrics if available
        if scanner.llm_client:
            await _persist_metrics(scanner.llm_client.get_llm_metrics())

        # Close resources (ensure both are attempted even if one fails)
        try:
            await scanner.close()
        except Exception as e:
            logger.error(f"Error closing scanner: {e}")

        try:
            await adapter.close()
        except Exception as e:
            logger.error(f"Error closing adapter: {e}")


@dataclass
class CheckOutcome:
    """Result of a local check run.

    `unanalyzed` is what separates "analyzed and clean" from "never analyzed" -
    a commit gate must not treat an unreachable LLM as a passing grade.
    """

    findings: list[Finding]
    unanalyzed: list[str]


def _common_root(paths: list[Path]) -> Path:
    """Directory that contains every path, for one consistent set of relatives."""
    if not paths:
        return Path.cwd()
    directories = [p if p.is_dir() else p.parent for p in paths]
    if len(directories) == 1:
        return directories[0]
    return Path(os.path.commonpath([str(d) for d in directories]))


async def _run_check(paths: tuple[str, ...], staged: bool, config_path: str | None) -> CheckOutcome:
    """Run local file check without platform API.

    Args:
        paths: Files and/or directories to check
        staged: Only check staged files
        config_path: Config file path (optional)

    Returns:
        CheckOutcome carrying the findings and the files that could not be
        analyzed
    """
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
        # Validate and resolve every path up front
        try:
            resolved = [Path(raw).resolve(strict=True) for raw in paths]
        except FileNotFoundError as exc:
            click.echo(f"Error: Path not found: {exc.filename}", err=True)
            raise SystemExit(1) from exc

        # Analysis is rooted at the common ancestor so the file paths handed to
        # the analyzers stay relative to one directory, whatever mix of files
        # and directories the caller (or a pre-push hook) passed.
        analysis_root = _common_root(resolved)

        if staged:
            # Get staged files from git index
            files = scanner.get_staged_files(str(analysis_root))
            # stderr: stdout is the machine-readable channel for --format json
            click.echo(f"Checking {len(files)} staged file(s)...", err=True)
        else:
            # Collected absolutely, then re-rooted once: a directory's targets
            # come back relative to that directory, which is only the same as
            # analysis_root when it is the sole argument.
            absolute: list[Path] = []
            for path_obj in resolved:
                if path_obj.is_file():
                    absolute.append(path_obj)
                else:
                    # Directory - get all .py and .md files
                    absolute.extend(
                        path_obj / rel for rel in scanner.get_scan_targets(str(path_obj))
                    )
            files = [str(p.relative_to(analysis_root)) for p in absolute]
            click.echo(f"Checking {len(files)} file(s)...", err=True)

        if not files:
            return CheckOutcome(findings=[], unanalyzed=[])

        # Analyze files
        all_findings: list[Finding] = []
        unanalyzed: set[str] = set()

        if config.llm and config.llm.enabled:
            # The two passes are independent; run them concurrently. The rate
            # limiter provides the back-pressure.
            code_result, docstring_result = await asyncio.gather(
                scanner.analyze_code_quality(
                    repo_path=str(analysis_root),
                    files=files,
                    repo_id="local",
                    commit_sha="local",
                ),
                scanner.analyze_docstrings(
                    repo_path=str(analysis_root),
                    files=files,
                    repo_id="local",
                    commit_sha="local",
                ),
            )
            for result in (code_result, docstring_result):
                all_findings.extend(result.findings)
                # Union, not sum: both passes run over the same files, so a
                # single unreachable endpoint fails each file twice.
                unanalyzed.update(result.failed_files)

        # Presentation belongs to the command, not the workflow
        return CheckOutcome(findings=all_findings, unanalyzed=sorted(unanalyzed))

    finally:
        # Cleanup
        await scanner.close()
