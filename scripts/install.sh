#!/usr/bin/env bash
#
# drep installer: add drep as a pre-push gate to the repository you are in.
#
#   curl -fsSL https://raw.githubusercontent.com/slb350/drep/main/scripts/install.sh | bash
#
# Bootstrap only. Everything that can damage a repository - editing the
# pre-commit config, touching core.hooksPath - lives in `drep init-hooks`,
# where it has tests; the provider presets live in `drep init-llm` and
# detection in `drep doctor`. What is left here genuinely needs a shell:
# finding a Python, installing the package, and getting onto PATH.
#
# Safe to re-run.

set -euo pipefail

[ -n "${BASH_VERSION:-}" ] || {
  echo "error: this script needs bash (try: curl ... | bash)" >&2
  exit 1
}

PACKAGE="drep-ai"

# Colour only when stdout is a terminal, so piping to a log stays readable.
if [ -t 1 ]; then
  BOLD=$'\033[1m'; YELLOW=$'\033[33m'; RED=$'\033[31m'; RESET=$'\033[0m'
else
  BOLD=""; YELLOW=""; RED=""; RESET=""
fi

say()  { printf '%s\n' "$*"; }
step() { printf '\n%s==> %s%s\n' "$BOLD" "$*" "$RESET"; }
warn() { printf '%swarning:%s %s\n' "$YELLOW" "$RESET" "$*" >&2; }
die()  { printf '%serror:%s %s\n' "$RED" "$RESET" "$*" >&2; exit 1; }

# --------------------------------------------------------------------------
step "Checking this is a git repository"
git rev-parse --show-toplevel >/dev/null 2>&1 \
  || die "Not inside a git repository. cd to the repo you want to gate, then re-run."
REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT"
say "  $REPO_ROOT"

# --------------------------------------------------------------------------
step "Installing drep and pre-commit"

# pip --user fails inside an active venv and on PEP 668 systems (Debian 12+,
# Fedora), so it is the last resort rather than the default.
install_with_pip() {
  command -v python3 >/dev/null 2>&1 || die "Need python3, pipx or uv to install drep."
  python3 -c 'import sys; sys.exit(0 if sys.version_info >= (3, 10) else 1)' \
    || die "drep needs Python 3.10 or newer (found $(python3 -V 2>&1))."
  python3 -m pip install --user --upgrade "$@" 2>/dev/null \
    || python3 -m pip install --upgrade "$@" \
    || die "pip could not install $*. Install pipx and re-run, or pip install it yourself."
}

install_tool() {
  local binary="$1" package="$2"
  if command -v "$binary" >/dev/null 2>&1; then
    say "  $binary: already installed"
  elif command -v pipx >/dev/null 2>&1; then
    # pipx isolates the CLI, which matters for something a git hook invokes
    # with an unpredictable PATH.
    pipx install "$package"
  elif command -v uv >/dev/null 2>&1; then
    uv tool install "$package"
  else
    warn "pipx not found; falling back to pip. pipx is recommended."
    install_with_pip "$package"
  fi
}

install_tool drep "$PACKAGE"
install_tool pre-commit pre-commit

# Checked after installing, not before: pip --user lands binaries in a
# directory that is not on PATH by default on macOS and Debian, and the next
# steps would fail confusingly.
for binary in drep pre-commit; do
  command -v "$binary" >/dev/null 2>&1 \
    || die "$binary installed but is not on PATH. Add your user bin directory to PATH and re-run."
done

# --------------------------------------------------------------------------
# The deterministic half - ruff, eslint, go vet, clippy - needs no model and
# no key, so "none" is a first-class answer here.
step "Choosing a model for the advisory review (optional)"
if [ ! -t 0 ] && [ ! -e /dev/tty ]; then
  say "  Non-interactive; skipping. Run 'drep init-llm --provider ...' later."
else
  say "  The deterministic checks run either way. This only adds LLM review."
  say ""
  say "    1) None        - deterministic tools only, no key, no cost"
  say "    2) Local       - LM Studio / Ollama on this machine"
  say "    3) OpenRouter  - one key, many models"
  say "    4) OpenAI      - directly against the OpenAI API"
  say ""
  printf 'Choose [1]: '
  # Read from the terminal, not stdin: under `curl | bash` stdin is the script.
  read -r choice < /dev/tty || choice=1
  provider=""
  case "${choice:-1}" in
    1|"") say "  Skipping - deterministic checks only." ;;
    2)    provider=local ;;
    3)    provider=openrouter ;;
    4)    provider=openai ;;
    *)    warn "Unrecognised choice '${choice}'; skipping model setup." ;;
  esac
  # Never fatal: the model is optional, and the hook is the point of running
  # this at all.
  if [ -n "$provider" ]; then
    drep init-llm --provider "$provider" || warn "Model setup failed; continuing without it."
  fi
fi

# --------------------------------------------------------------------------
step "Installing the pre-push hook"
# Owns the pre-commit config merge and the core.hooksPath handling, both of
# which have tests. Doing either in shell went wrong in ways that silently
# disabled every hook in the repository.
drep init-hooks

# --------------------------------------------------------------------------
step "What drep will check here"
drep doctor || true

step "Done"
say "drep runs on 'git push'. To see it now without pushing:"
say "  drep check ."
