#!/usr/bin/env bash
#
# Run the mutation sweep on a bigger machine over SSH, then report its verdict
# here.
#
# Mutation testing is the most CPU-hungry gate in this repo: every mutant is a
# full build plus a full test run, and the hook fires on a laptop the developer
# is still using. Measured on 12 mutants from src/docs/fence.rs: local M5 Max at
# -j 4 takes 1m54 with the machine pinned; strix at -j 8 takes 54s and costs
# this machine nothing.
#
# More jobs is not better. On the same scope strix measured 38s at -j 4, 54s at
# -j 8 and 72s at -j 16, at 705-2200% CPU of a possible 3200 - each mutant is
# its own `cargo` in its own copy of the tree, so the jobs oversubscribe rather
# than parallelise, and the run is never CPU-bound. Timings drift by a third
# between runs on a shared box, so treat MUTANTS_JOBS as a knob to measure, not
# a number to raise on principle.
#
# The verdict rule is NOT duplicated here. This script syncs, invokes
# scripts/mutants-run.sh on the remote, and propagates its exit code - so the
# hook, CI and the remote sweep cannot disagree about what counts as a failure.
#
# Falls back to a local run, loudly, when the host is unreachable. A commit gate
# that silently skips itself because the LAN blipped is worse than a slow one.
#
#   DREP_MUTANTS_HOST    ssh target (default: strix.local)
#   DREP_MUTANTS_DIR     remote path, $HOME-relative (default: ci/<repo name>)
#   DREP_MUTANTS_REMOTE  0 to force a local run
#   MUTANTS_JOBS         -j for the remote run (default: 8)
#   MUTANTS_LOCAL_JOBS   -j for a local or fallback run (default: 4)

set -euo pipefail

cd "$(dirname "$0")/.."

HOST="${DREP_MUTANTS_HOST:-strix.local}"
REMOTE_DIR="${DREP_MUTANTS_DIR:-ci/$(basename "$PWD")}"
JOBS="${MUTANTS_JOBS:-8}"

run_local() {
  MUTANTS_JOBS="${MUTANTS_LOCAL_JOBS:-4}" exec ./scripts/mutants-run.sh "$@"
}

if [ "${DREP_MUTANTS_REMOTE:-1}" = "0" ]; then
  run_local "$@"
fi

if ! ssh -o BatchMode=yes -o ConnectTimeout=5 "$HOST" true 2>/dev/null; then
  echo "warning: $HOST is unreachable - running the mutation sweep locally instead." >&2
  echo "         This will use this machine's CPU for the duration." >&2
  run_local "$@"
fi

echo "mutants: running on $HOST (-j $JOBS), results mirrored back to target/mutants"

# --delete so a file deleted locally cannot linger and be mutated remotely.
# target/ is excluded in both directions: the remote keeps its own, which is
# what makes the second run incremental. Credentials are excluded because
# nothing in the suite reads them and they have no business on another host.
ssh -o BatchMode=yes "$HOST" "mkdir -p ~/'$REMOTE_DIR'"
rsync -a --delete \
  --exclude target --exclude mutants.out \
  --exclude .git --exclude venv --exclude node_modules \
  --exclude '.tokens' --exclude '.env*' \
  ./ "$HOST:$REMOTE_DIR/"

# `--in-diff <file>` names a path the remote also needs, and it lives under
# target/ - which the sync above deliberately skips. Ship it by itself.
args=("$@")
for ((i = 0; i < ${#args[@]}; i++)); do
  [ "${args[$i]}" = "--in-diff" ] || continue
  diff="${args[$((i + 1))]}"
  case "$diff" in
    /*) echo "mutants-remote: --in-diff needs a repo-relative path, got $diff" >&2; exit 64 ;;
  esac
  ssh -o BatchMode=yes "$HOST" "mkdir -p ~/'$REMOTE_DIR/$(dirname "$diff")'"
  rsync -a "$diff" "$HOST:$REMOTE_DIR/$diff"
done

# flock serialises two commits racing for the same remote tree; they would
# otherwise share one target/ and one results directory. -w so a stuck run
# cannot block a commit forever.
# `bash -s` rather than a quoted one-liner: the arguments are quoted with
# printf %q, which is bash's dialect, so the remote end must be bash whatever
# login shell the account uses.
status=0
# shellcheck disable=SC2087  # local expansion is the point: the remote dir,
# the job count and the %q-quoted arguments are all known here. \$HOME is
# escaped so it resolves there.
ssh -o BatchMode=yes "$HOST" bash -s <<EOF || status=$?
set -euo pipefail
export PATH=\$HOME/.cargo/bin:\$PATH
cd ~/'$REMOTE_DIR'
mkdir -p target/mutants
MUTANTS_JOBS=$JOBS flock -w 1800 target/mutants ./scripts/mutants-run.sh $(printf '%q ' "$@")
EOF

# Mirror the results back so `missed.txt`, the logs and the diffs of surviving
# mutants can be read here, where the fix gets written.
mkdir -p target/mutants
rsync -a "$HOST:$REMOTE_DIR/target/mutants/" target/mutants/ 2>/dev/null || true

exit "$status"
