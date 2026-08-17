"""The lint-docs command: rule-based markdown checks.

Split from drep.cli to keep that module under the project's file-size limit,
and because this command shares nothing with the rest: no LLM, no config, no
platform token, and deliberately none of the heavy imports the other commands
pull in - it runs on every commit as a pre-commit hook.
"""

import asyncio
import json

import click

from drep.cli import cli
from drep.core.file_targets import expand_paths, is_markdown
from drep.documentation.analyzer import DocumentationAnalyzer
from drep.models.config import DocumentationConfig


@cli.command()
@click.argument("paths", nargs=-1, type=click.Path(exists=True))
@click.option("--output", type=click.Choice(["text", "json"]), default="text", help="Output format")
@click.option("--strict", is_flag=True, help="Exit non-zero when issues are found (pre-commit)")
def lint_docs(paths, output, strict):
    """Lint markdown documentation files for style and formatting issues.

    Accepts any number of files or directories; defaults to the current
    directory. Rule-based only - no LLM, no config, no platform token.

    Examples:
        drep lint-docs docs/
        drep lint-docs README.md CHANGELOG.md
        drep lint-docs docs/ --output json
        drep lint-docs --strict          # block a commit on issues
    """
    # Create analyzer with markdown checks enabled
    config = DocumentationConfig(enabled=True, markdown_checks=True)
    analyzer = DocumentationAnalyzer(config)

    md_files = expand_paths(paths or (".",), is_markdown)

    if not md_files:
        click.echo("No markdown files found.")
        return

    async def analyze_all():
        """One event loop for the whole run, not one per file."""
        results = []
        for md_file in md_files:
            try:
                content = md_file.read_text(encoding="utf-8")
                findings = await analyzer.analyze_file(str(md_file), content)

                if findings.pattern_issues:
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
        return results

    results = asyncio.run(analyze_all())
    total_issues = sum(len(findings.pattern_issues) for _, findings in results)

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

    if strict and total_issues:
        raise SystemExit(1)
