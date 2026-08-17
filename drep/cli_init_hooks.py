"""The init-hooks command: wire drep into this repository's git hooks.

This is the only part of installing drep that can damage something. It edits
the user's pre-commit config and touches git's hook configuration, so it lives
here rather than in the installer shell script - every failure mode below has a
test, which a curl-piped script cannot have.

Two of those failure modes are silent, which is why they are handled so
carefully:

- `core.hooksPath` set to an empty string disables **every** hook in the repo,
  and reads back as absent - so a later run cannot tell the repo is broken.
  It is therefore always restored, including when the install fails.
- With `core.hooksPath` set at all, git ignores `.git/hooks` completely, so a
  repo-local hook never fires. A chainer in the global directory is what keeps
  it working.
"""

import contextlib
import shutil
import subprocess
from collections.abc import Iterator
from pathlib import Path

import click
import yaml

from drep import __version__

REPO_URL = "https://github.com/slb350/drep"
DEFAULT_HOOK_ID = "drep-check-push"

# `git rev-parse --git-common-dir`, not `$REPO/.git`: in a linked worktree or a
# submodule `.git` is a file, so the literal path does not exist and the hook
# silently never runs. Not `--git-path hooks/pre-push` either - that honours
# core.hooksPath and would make the chainer exec itself.
_CHAINER = """#!/bin/bash
# Chains to the repo-local pre-push hook, which git ignores while
# core.hooksPath is set. `exec` matters twice: it keeps the local hook's exit
# status (that is what aborts the push) and hands over stdin unread, which is
# how git delivers the refs being pushed.
LOCAL_HOOK="$(git rev-parse --git-common-dir)/hooks/pre-push"
if [[ -x "$LOCAL_HOOK" ]]; then
    exec "$LOCAL_HOOK" "$@"
fi
"""


def _git(root: Path, *args: str) -> subprocess.CompletedProcess:
    return subprocess.run(["git", *args], cwd=root, capture_output=True, text=True, check=False)


def _run_pre_commit_install(root: Path) -> None:
    """Install the pre-push hook. Separate so tests can make it fail."""
    executable = shutil.which("pre-commit")
    if executable is None:
        raise RuntimeError("pre-commit is not installed or not on PATH")
    result = subprocess.run(
        [executable, "install", "--hook-type", "pre-push"],
        cwd=root,
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode != 0:
        raise RuntimeError(f"pre-commit install failed: {result.stderr.strip()[:200]}")


@contextlib.contextmanager
def _hooks_path_suspended(root: Path) -> Iterator[None]:
    """Let pre-commit install while core.hooksPath is set, then put it back.

    pre-commit refuses to install at all while the setting exists, and an empty
    local override satisfies it. That override also disables every hook in the
    repo, so restoring it is done in a finally: an exception here previously
    left the repository with no working hooks and no way to notice.
    """
    had_local = _git(root, "config", "--local", "--get", "core.hooksPath")
    previous = had_local.stdout.rstrip("\n") if had_local.returncode == 0 else None

    _git(root, "config", "--local", "core.hooksPath", "")
    try:
        yield
    finally:
        if previous is None:
            _git(root, "config", "--local", "--unset", "core.hooksPath")
        else:
            # Restore the repo's own value verbatim rather than deleting it.
            _git(root, "config", "--local", "core.hooksPath", previous)


def _resolve_hooks_dir(root: Path, hooks_path: str) -> Path:
    """core.hooksPath, resolved the way git resolves it.

    A relative value is relative to the *repository*, not the current working
    directory - resolving it against the cwd wrote a chainer into whatever
    directory the caller happened to be in.
    """
    candidate = Path(hooks_path)
    return candidate if candidate.is_absolute() else root / candidate


def _ensure_chainer(root: Path, hooks_path: Path) -> None:
    """Make sure the global hooks dir forwards pre-push to the repo."""
    chainer = hooks_path / "pre-push"

    if chainer.exists():
        body = chainer.read_text(errors="replace")
        if "hooks/pre-push" not in body:
            click.echo(
                f"  {chainer} exists but does not appear to chain to the repo-local hook.\n"
                "  drep will not run on push until it does - see the snippet in the docs.",
                err=True,
            )
        elif not chainer.stat().st_mode & 0o111:
            # git ignores a non-executable hook, silently
            click.echo(f"  {chainer} is not executable; making it so.")
            chainer.chmod(0o755)
        return

    hooks_path.mkdir(parents=True, exist_ok=True)
    chainer.write_text(_CHAINER)
    chainer.chmod(0o755)
    click.echo(f"  Wrote a chainer at {chainer}")


def _merge_config(path: Path) -> bool:
    """Add drep's hook to a pre-commit config, preserving everything else.

    Parsed and rewritten rather than appended: appending text produced invalid
    YAML against pre-commit's own zero-indent style, and against any config
    with a top-level key after `repos:`.

    Returns:
        True if the file was changed.
    """
    config: dict = {}
    if path.exists():
        try:
            config = yaml.safe_load(path.read_text()) or {}
        except yaml.YAMLError as exc:
            raise click.ClickException(
                f"{path} could not be parsed, so it will not be modified:\n  {exc}"
            ) from exc
        if not isinstance(config, dict):
            raise click.ClickException(f"{path} could not be parsed as a mapping.")

    repos = config.setdefault("repos", [])
    if any(REPO_URL in str(entry.get("repo", "")) for entry in repos):
        return False

    repos.append(
        {
            "repo": REPO_URL,
            "rev": f"v{__version__}",
            "hooks": [{"id": DEFAULT_HOOK_ID}],
        }
    )
    path.write_text(yaml.dump(config, sort_keys=False))
    return True


@click.command(name="init-hooks")
@click.option("--path", "path_arg", default=".", help="Repository to install into")
@click.option("--skip-install", is_flag=True, help="Write config but do not run pre-commit")
def init_hooks(path_arg, skip_install):
    """Install drep as a pre-push gate in this repository.

    Examples:
        drep init-hooks
        drep init-hooks --path ../other-repo
    """
    root = Path(path_arg).resolve()

    toplevel = _git(root, "rev-parse", "--show-toplevel")
    if toplevel.returncode != 0:
        click.echo(f"Error: {root} is not inside a git repository.", err=True)
        raise SystemExit(1)
    root = Path(toplevel.stdout.strip())

    if _merge_config(root / ".pre-commit-config.yaml"):
        click.echo(f"✓ Added {DEFAULT_HOOK_ID} to .pre-commit-config.yaml")
    else:
        click.echo("  .pre-commit-config.yaml already references drep; left alone.")

    # --type=path so git expands ~ and ~user itself; hand-rolled expansion
    # mangled `~alice/hooks`, and bare $HOME is unset in some environments.
    configured = _git(root, "config", "--get", "--type=path", "core.hooksPath")
    hooks_path = configured.stdout.strip() if configured.returncode == 0 else ""

    if skip_install:
        if hooks_path:
            _ensure_chainer(root, _resolve_hooks_dir(root, hooks_path))
        click.echo("  Skipping `pre-commit install` (--skip-install).")
        return

    if hooks_path:
        click.echo(f"  core.hooksPath is set to {hooks_path}")
        click.echo("  git looks there and not in .git/hooks, so a repo hook needs a chainer.")
        _ensure_chainer(root, _resolve_hooks_dir(root, hooks_path))
        with _hooks_path_suspended(root):
            _run_pre_commit_install(root)
    else:
        _run_pre_commit_install(root)

    click.echo("✓ drep will run on `git push`")
