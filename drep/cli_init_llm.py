"""The init-llm command: point drep at a model, and nothing else.

`drep init` is the full wizard and asks for platform credentials, which a
local pre-push gate never uses. This writes only the `llm` block, from a named
preset, so the installer can configure a model in one non-interactive call -
and so the preset table stays in Python instead of being duplicated in shell.
"""

from pathlib import Path

import click
import yaml
from pydantic import ValidationError

from drep.models.config import LLMConfig
from drep.models.llm_presets import LLM_PRESETS, preset_names


@click.command(name="init-llm")
@click.option(
    "--provider",
    type=click.Choice(preset_names()),
    required=True,
    help="Which model provider to configure",
)
@click.option("--model", default=None, help="Model name (defaults to the preset's)")
@click.option("--endpoint", default=None, help="Base URL (required for --provider custom)")
@click.option("--config", "config_path", default="config.yaml", help="Config file to write")
@click.option("--force", is_flag=True, help="Overwrite an existing llm section")
def init_llm(provider, model, endpoint, config_path, force):
    """Configure which model drep uses, without the full wizard.

    Examples:
        drep init-llm --provider local
        drep init-llm --provider openrouter
        drep init-llm --provider custom --endpoint https://host/v1 --model m
    """
    preset = LLM_PRESETS[provider]

    resolved_endpoint = endpoint or preset.endpoint
    if not resolved_endpoint:
        click.echo(
            f"Error: --provider {provider} needs an --endpoint (it presumes no host).",
            err=True,
        )
        raise SystemExit(1)

    resolved_model = model or preset.default_model
    if not resolved_model:
        click.echo(f"Error: --provider {provider} needs a --model.", err=True)
        raise SystemExit(1)

    path = Path(config_path)
    existing = {}
    if path.exists():
        existing = yaml.safe_load(path.read_text()) or {}
        if existing.get("llm") and not force:
            click.echo(
                f"Error: {path} already configures an llm section. "
                "Re-run with --force to replace it.",
                err=True,
            )
            raise SystemExit(1)

    # Other sections are the user's; only llm is ours to write.
    existing["llm"] = preset.to_config(model=resolved_model, endpoint=resolved_endpoint)

    # Validated before it lands: an unloadable config written here would only
    # surface much later, inside `drep check`.
    try:
        LLMConfig(**{k: v for k, v in existing["llm"].items() if k != "api_key"})
    except ValidationError as exc:
        click.echo(f"Error: that would produce an invalid config:\n{exc}", err=True)
        raise SystemExit(1) from exc

    path.write_text(yaml.dump(existing, sort_keys=False))

    click.echo(f"✓ {path} now uses {preset.display_name} ({resolved_model})")

    if preset.api_key_env:
        # The key itself is never written: config.yaml is committed in most
        # repos, so only the variable name goes in.
        click.echo(f"\nSet your key before running drep:\n  export {preset.api_key_env}='...'")
