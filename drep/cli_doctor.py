"""The doctor command: what will drep actually do in this repository?

Adoption question, not a debugging one. A user adding drep as a gate wants to
know their real coverage before they trust it - which languages are here,
which of their own tools will run, and which are configured but missing. The
installer prints this so the answer is concrete rather than a promise.

Deliberately never fails: this is diagnosis. `drep check` is the gate.
"""

from pathlib import Path

import click

from drep.config import find_config_file, load_config
from drep.core.file_targets import expand_paths, is_scan_target
from drep.languages import registry
from drep.languages.runner import is_configured, resolve_tool


@click.command()
@click.argument("path", default=".", type=click.Path(exists=True))
def doctor(path):
    """Report which languages and tools drep will use here.

    Examples:
        drep doctor
        drep doctor path/to/repo
    """
    root = Path(path).resolve()
    files = [str(p.relative_to(root)) for p in expand_paths([root], is_scan_target)]

    click.echo(f"drep in {root}")
    click.echo("=" * 60)

    languages = registry.detect_all(files)
    if not languages:
        click.echo("\nNo source files drep recognises were found here.")
        return

    click.echo("\nLanguages found:")
    for language in languages:
        owned = sum(1 for f in files if registry.detect(f) is language)
        click.echo(f"  {language.display_name}: {owned} file(s)")

    click.echo("\nDeterministic checks (these gate):")
    missing = []
    for language in languages:
        if not language.tools:
            click.echo(f"  {language.display_name}: no tools wired up yet")
            continue
        for spec in language.tools:
            if not is_configured(spec, root):
                click.echo(
                    f"  {spec.name}: not configured "
                    f"(add one of: {', '.join(spec.config_files[:3])})"
                )
            elif resolve_tool(spec, root) is None:
                click.echo(
                    f"  {spec.name}: configured but NOT INSTALLED - these checks will not run"
                )
                missing.append(spec.name)
            else:
                click.echo(f"  {spec.name}: ready")

    _report_llm()

    if missing:
        click.echo(
            f"\n{len(missing)} configured tool(s) are missing: {', '.join(missing)}. "
            "drep exits 2 rather than reporting those files clean."
        )


def _report_llm() -> None:
    """Say whether the semantic half is available, and that it is optional."""
    click.echo("\nLLM analysis (advisory, does not gate):")

    config_file = find_config_file(None)
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
