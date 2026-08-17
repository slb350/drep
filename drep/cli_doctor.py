"""The doctor command: what will drep actually do in this repository?

Adoption question, not a debugging one. A user adding drep as a gate wants to
know their real coverage before they trust it - which languages are here,
which of their own tools will run, and which are configured but missing. The
installer prints this so the answer is concrete rather than a promise.

Deliberately never fails: this is diagnosis. `drep check` is the gate.
"""

import os
import re
from pathlib import Path

import click

from drep.config import find_config_file, load_config
from drep.core.file_targets import expand_paths, is_scan_target
from drep.languages import registry
from drep.languages.runner import tool_status
from drep.languages.workflow import group_by_language


@click.command()
@click.argument("path", default=".", type=click.Path(exists=True))
@click.option("--config", "config_path", default=None, help="Config file to report on")
def doctor(path, config_path):
    """Report which languages and tools drep will use here.

    Examples:
        drep doctor
        drep doctor path/to/repo
    """
    root = Path(path).resolve()
    files = [str(p.relative_to(root)) for p in expand_paths([root], is_scan_target)]

    click.echo(f"drep in {root}")
    click.echo("=" * 60)

    # The same bucketing `drep check` uses, so the counts are a measurement
    # rather than a claim - and one pass rather than one per language.
    grouped = group_by_language(files)
    if not grouped:
        click.echo("\nNo source files drep recognises were found here.")
        return

    languages = [registry.get(name) for name in grouped]
    click.echo("\nLanguages found:")
    for language in languages:
        click.echo(f"  {language.display_name}: {len(grouped[language.name])} file(s)")

    click.echo("\nDeterministic checks (these gate):")
    missing = []
    for language in languages:
        if not language.tools:
            click.echo(f"  {language.display_name}: no tools wired up yet")
            continue
        for spec in language.tools:
            # Asks the runner rather than re-deriving: doctor must not be able
            # to say "ready" for a tool that check will skip.
            status = tool_status(spec, root)
            click.echo(f"  {spec.name}: {status.detail}")
            if status.status == "unavailable":
                missing.append(spec.name)

    _report_llm(root, config_path)

    if missing:
        click.echo(
            f"\n{len(missing)} configured tool(s) are missing: {', '.join(missing)}. "
            "drep exits 2 rather than reporting those files clean."
        )


def _report_llm(root: Path, config_path: str | None) -> None:
    """Say whether the semantic half is available, and that it is optional."""
    click.echo("\nLLM analysis (advisory, does not gate):")

    # Resolved relative to the directory being reported on, not the cwd:
    # `drep doctor other/repo` described other/repo's languages against this
    # directory's config.
    config_file = Path(config_path) if config_path else root / "config.yaml"
    if not config_file.exists():
        config_file = find_config_file(config_path)
    if not config_file.exists():
        click.echo("  No config file - LLM analysis is off.")
        click.echo("  The deterministic checks above still run. `drep init` to add a model.")
        return

    try:
        config = load_config(str(config_file), require_platform=False)
    except Exception as exc:  # diagnosis reports problems, it never raises them
        click.echo(f"  Config at {config_file} could not be loaded: {str(exc)[:120]}")
        return

    if not (config.llm and config.llm.enabled):
        click.echo("  Disabled in config. The deterministic checks above still run.")
        return

    click.echo(f"  {config.llm.model} at {config.llm.endpoint}")

    # The commonest failure after `drep init-llm` is the key never being
    # exported. The tool half reports readiness; this half should too.
    _report_api_key(config_file)


def _report_api_key(config_file: Path) -> None:
    """Warn if the config references an env var that is not set."""
    raw = config_file.read_text()
    for var in re.findall(r"\$\{([A-Z_][A-Z0-9_]*)\}", raw):
        if not os.environ.get(var):
            click.echo(f"  {var} is NOT set - LLM analysis will fail until you export it.")
