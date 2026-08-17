#!/usr/bin/env bash
#
# drep installer: add drep as a pre-push gate to the repository you are in.
#
#   curl -fsSL https://raw.githubusercontent.com/slb350/drep/main/scripts/install.sh | bash
#
# Deliberately thin. Everything it can delegate to drep itself, it does:
# `drep init-llm` owns the provider presets and `drep doctor` owns language
# and tool detection, so this script never duplicates that knowledge and
# cannot drift from it. What is left is genuinely shell-shaped: finding a
# Python, installing the package, and working around git's core.hooksPath.
#
# Safe to re-run: every step checks before it writes.

set -euo pipefail

REPO_URL="https://github.com/slb350/drep"
PACKAGE="drep-ai"

say()  { printf '%s\n' "$*"; }
step() { printf '\n\033[1m==> %s\033[0m\n' "$*"; }
warn() { printf '\033[33mwarning:\033[0m %s\n' "$*" >&2; }
die()  { printf '\033[31merror:\033[0m %s\n' "$*" >&2; exit 1; }

# --------------------------------------------------------------------------
# 0. Preconditions
# --------------------------------------------------------------------------
step "Checking this is a git repository"
git rev-parse --show-toplevel >/dev/null 2>&1 \
  || die "Not inside a git repository. cd to the repo you want to gate, then re-run."
REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT"
say "  $REPO_ROOT"

# --------------------------------------------------------------------------
# 1. drep itself
# --------------------------------------------------------------------------
step "Installing drep"
if command -v drep >/dev/null 2>&1; then
  say "  Already installed: $(drep --version 2>/dev/null || echo 'drep')"
elif command -v pipx >/dev/null 2>&1; then
  # pipx keeps drep in its own environment, which is what you want for a CLI
  # that will be invoked from a git hook with an unpredictable PATH.
  pipx install "$PACKAGE"
elif command -v uv >/dev/null 2>&1; then
  uv tool install "$PACKAGE"
else
  command -v python3 >/dev/null 2>&1 || die "Need python3, pipx or uv to install drep."
  python3 -c 'import sys; sys.exit(0 if sys.version_info >= (3, 10) else 1)' \
    || die "drep needs Python 3.10 or newer (found $(python3 -V))."
  warn "pipx not found; installing with pip --user. pipx is recommended."
  python3 -m pip install --user --upgrade "$PACKAGE"
fi

command -v drep >/dev/null 2>&1 \
  || die "drep installed but is not on PATH. Add your user bin directory to PATH and re-run."

# --------------------------------------------------------------------------
# 2. Which model, if any
# --------------------------------------------------------------------------
# The deterministic half - ruff, eslint, go vet, clippy - needs no model and
# no key. The LLM half is advisory and entirely optional, so this asks rather
# than assumes, and "none" is a first-class answer.
step "Choosing a model for the advisory analysis (optional)"
if [ -f config.yaml ] && grep -q '^llm:' config.yaml 2>/dev/null; then
  say "  config.yaml already configures a model; leaving it alone."
elif [ ! -t 0 ]; then
  # Piped from curl with no tty: skip rather than hang on a prompt.
  say "  Non-interactive install; skipping. Run 'drep init-llm --provider ...' later."
else
  say "  The deterministic checks below run either way. This only adds LLM review."
  say ""
  say "    1) None            - deterministic tools only, no key, no cost"
  say "    2) Local           - LM Studio / Ollama on this machine"
  say "    3) OpenRouter      - one key, many models"
  say "    4) OpenAI          - directly against the OpenAI API"
  say ""
  printf 'Choose [1]: '
  read -r choice || choice=1
  case "${choice:-1}" in
    1|"") say "  Skipping - deterministic checks only." ;;
    2)    drep init-llm --provider local ;;
    3)    drep init-llm --provider openrouter ;;
    4)    drep init-llm --provider openai ;;
    *)    warn "Unrecognised choice '${choice}'; skipping model setup." ;;
  esac
fi

# --------------------------------------------------------------------------
# 3. The hook
# --------------------------------------------------------------------------
step "Installing the pre-push hook"
command -v pre-commit >/dev/null 2>&1 || {
  if command -v pipx >/dev/null 2>&1; then pipx install pre-commit
  else python3 -m pip install --user --upgrade pre-commit
  fi
}

if [ -f .pre-commit-config.yaml ] && grep -q 'drep' .pre-commit-config.yaml; then
  say "  .pre-commit-config.yaml already references drep; leaving it alone."
else
  if [ -f .pre-commit-config.yaml ]; then
    warn "Appending to your existing .pre-commit-config.yaml - review the result."
    cat >> .pre-commit-config.yaml <<YAML

  - repo: ${REPO_URL}
    rev: main
    hooks:
      - id: drep-check-push
YAML
  else
    cat > .pre-commit-config.yaml <<YAML
repos:
  - repo: ${REPO_URL}
    rev: main
    hooks:
      # Runs at pre-push: the deterministic tools this project already
      # configures gate the push, and LLM findings (if a model is set up)
      # are reported without blocking.
      - id: drep-check-push
YAML
  fi
  say "  Wrote .pre-commit-config.yaml"
fi

# git consults core.hooksPath INSTEAD of .git/hooks when it is set, so a
# repo-local hook silently never fires - and pre-commit refuses to install at
# all while it is set. Both failure modes are quiet, which is why this is
# handled rather than documented.
HOOKS_PATH="$(git config --get core.hooksPath || true)"
if [ -n "$HOOKS_PATH" ]; then
  warn "core.hooksPath is set to '${HOOKS_PATH}'."
  say  "  git will look there and not in .git/hooks, so a repo-local hook would never run."
  EXPANDED="${HOOKS_PATH/#\~/$HOME}"
  if [ -e "${EXPANDED}/pre-push" ]; then
    say "  ${EXPANDED}/pre-push already exists - assuming it chains to the repo hook."
  else
    say "  Writing a chainer at ${EXPANDED}/pre-push so repo-local hooks still run."
    mkdir -p "$EXPANDED"
    cat > "${EXPANDED}/pre-push" <<'CHAINER'
#!/bin/bash
# Chains to the repo-local pre-push hook, which git would otherwise ignore
# because core.hooksPath is set. `exec` matters twice: it keeps the local
# hook's exit status (that is what aborts the push) and hands over stdin
# unread, which is how git delivers the refs being pushed.
REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null)"
LOCAL_HOOK="$REPO_ROOT/.git/hooks/pre-push"
if [[ -x "$LOCAL_HOOK" ]]; then
    exec "$LOCAL_HOOK" "$@"
fi
CHAINER
    chmod +x "${EXPANDED}/pre-push"
  fi
  # pre-commit refuses while hooksPath is set; an empty repo-local override
  # satisfies it, and is removed immediately so the global chain is untouched.
  git config --local core.hooksPath ""
  pre-commit install --hook-type pre-push
  git config --local --unset core.hooksPath
else
  pre-commit install --hook-type pre-push
fi

# --------------------------------------------------------------------------
# 4. What this repository will actually get
# --------------------------------------------------------------------------
step "What drep will check here"
drep doctor || true

step "Done"
say "drep runs on 'git push'. To see it now without pushing:"
say "  drep check ."
