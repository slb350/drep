#!/usr/bin/env bash
#
# Tests for the SSH session pipes in scripts/mutants-remote.sh. They are made in one session dir under the scratch root, or under DREP_MUTANTS_TMPDIR, no pipe can be reached by path from the start of the handshake on, and no session dir survives a run that succeeds, fails, fails at mkfifo, or was killed earlier, and a stale one that cannot be removed stops the run. The ssh, rsync and mkfifo stand-ins on PATH keep every case on this machine, and every wait is bounded, so a protocol regression fails instead of hanging.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

CASE=setup
# Bash 3.2, which macOS ships, does not stop a set -e script when `[[ ]]` fails, so every assertion names its own failure instead of relying on errexit.
fail() {
  echo "mutants-remote ($CASE): $1" >&2
  exit 1
}

# An outer repository's git environment, a hook's included, would steer the stand-in checkout's git. This is the list git reports, as the Rust runner's scrub uses.
# shellcheck disable=SC2046  # One variable name per word.
unset $(git rev-parse --local-env-vars)
# The mutation scripts and the stand-ins read these, so a case sets each for its own run only. An inherited host lock or result token would make the run act on its parent's.
unset DREP_MUTANTS_TMPDIR DREP_MUTANTS_REMOTE MUTANTS_OUT_DIR MUTANTS_SOURCE_DIR MUTANTS_EXTRA_FILES MUTANTS_JOBS MUTANTS_LOCAL_JOBS DREP_MUTANTS_HOST_LOCK DREP_MUTANTS_RSYNC_TIMEOUT_SECONDS DREP_MUTANTS_RESULT_TOKEN STUB_RUN_STATUS STUB_MKFIFO_FAIL

# Found before PATH gains the stand-ins, so the mkfifo stand-in can run the real one.
REAL_MKFIFO="$(command -v mkfifo)"

DIR="$(mktemp -d)"
# A case makes a session dir unwritable, so permissions are restored before the tree is removed.
trap 'chmod -R u+rwX "$DIR" 2>/dev/null || true; ( . "$ROOT/scripts/mutants-common.sh" && remove_tree "$DIR" )' EXIT
# mktemp answers through the /var symlink while the script's git rev-parse answers canonically, so the paths the test compares are canonical.
DIR="$(cd "$DIR" && pwd -P)"
REPO="$DIR/repo"
SCRATCH="$REPO.mutants-tmp"
BIN="$DIR/bin"
LOG="$DIR/run.log"
SNAPSHOT="$DIR/snapshot"
MKFIFO_LOG="$DIR/mkfifo.log"

# A real disposable checkout holding copies of the mutation scripts; the run's scratch root lands beside it.
mkdir -p "$REPO/scripts" "$BIN"
cp "$ROOT/scripts/mutants-remote.sh" "$ROOT/scripts/mutants-common.sh" "$ROOT/scripts/mutants-ai1-transport.sh" "$REPO/scripts/"
git init -q "$REPO"

# write_standin NAME writes $BIN/NAME from the heredoc on stdin, after lines that fix DIR and REAL_MKFIFO, so a stand-in reads nothing from the environment but a case's switches.
write_standin() {
  {
    printf '#!/usr/bin/env bash\nDIR=%q\nREAL_MKFIFO=%q\n' "$DIR" "$REAL_MKFIFO"
    cat
  } >"$BIN/$1"
  chmod +x "$BIN/$1"
}

# Answers the reachability probe and plays the remote side of the protocol, recording every pipe under DIR while the run is under way. The command text is quoted twice on its way here, so the token is found by its shape.
write_standin ssh <<'EOF'
command_text="${*: -1}"
case "$command_text" in
*" true") exit 0 ;;
esac
[[ "$command_text" =~ ([0-9]+-[0-9]+-[0-9]+) ]] || exit 74
token="${BASH_REMATCH[1]}"
printf 'mutants-lock-ready:%s\n' "$token"
IFS= read -r -t 20 action && [[ $action == run ]] || exit 74
find "$DIR" -type p >>"$DIR/snapshot" || { echo "find failed" >>"$DIR/snapshot"; exit 74; }
status="${STUB_RUN_STATUS:-0}"
printf 'mutants-run-finished:%s:%s\n' "$token" "$status"
IFS= read -r -t 20 action && [[ $action == mirrored ]] || exit 74
exit "$status"
EOF

# Copies nothing, and records every pipe under DIR at each transfer: the source push is the first thing after the handshake opens.
write_standin rsync <<'EOF'
find "$DIR" -type p >>"$DIR/snapshot" || { echo "find failed" >>"$DIR/snapshot"; exit 74; }
EOF

# Logs the paths it is asked for, then fails when the case says so or makes them.
write_standin mkfifo <<'EOF'
printf '%s\n' "$@" >>"$DIR/mkfifo.log"
[[ ${STUB_MKFIFO_FAIL:-0} != 1 ]] || exit 1
exec "$REAL_MKFIFO" "$@"
EOF

# run_remote EXPECTED_STATUS [NAME=VALUE]... runs the checkout's script with the stand-ins first on PATH and the given environment, and fails unless it exits EXPECTED_STATUS. perl's alarm bounds the whole run, and the lock wait is zero, so a regression that blocks fails within a minute.
run_remote() {
  local expected="$1" status=0
  shift
  rm -f "$SNAPSHOT" "$MKFIFO_LOG"
  (
    cd "$REPO" || exit 1
    PATH="$BIN:$PATH" env DREP_MUTANTS_HOST_LOCK_WAIT_SECONDS=0 "$@" \
      perl -e 'alarm shift; exec @ARGV or exit 127' 60 bash ./scripts/mutants-remote.sh
  ) </dev/null >"$LOG" 2>&1 || status=$?
  [[ $status -eq $expected ]] || fail "the run ended with $status, not $expected: $(<"$LOG")"
}

assert_no_session_left() {
  local left
  left="$(find "$DIR" -name 'remote-session-*')"
  [[ -z $left ]] || fail "session dirs were left behind: $left"
}

# assert_session_under ROOT fails unless the run made exactly its two pipes in one session dir under ROOT, the stand-ins found no pipe under DIR once the handshake began, and no session dir is left.
assert_session_under() {
  local root="$1" session
  [[ -f $MKFIFO_LOG && -f $SNAPSHOT ]] || fail "the run never reached mkfifo or the transfers: $(<"$LOG")"
  session="$(head -n 1 "$MKFIFO_LOG")"
  session="${session%/control-in}"
  [[ $session == "$root"/remote-session-* && "$(<"$MKFIFO_LOG")" == "$session/control-in"$'\n'"$session/control-out" ]] ||
    fail "the pipes were not made in one session dir under $root: $(<"$MKFIFO_LOG")"
  [[ ! -s $SNAPSHOT ]] || fail "a pipe could be reached by path during the run: $(<"$SNAPSHOT")"
  assert_no_session_left
}

CASE="a run that succeeds"
run_remote 0
assert_session_under "$SCRATCH"
echo "ok $CASE"

CASE="a run whose mutation fails"
run_remote 7 STUB_RUN_STATUS=7
assert_session_under "$SCRATCH"
echo "ok $CASE"

CASE="DREP_MUTANTS_TMPDIR moves the session"
mkdir -p "$DIR/tmpdir"
run_remote 0 DREP_MUTANTS_TMPDIR="$DIR/tmpdir"
assert_session_under "$DIR/tmpdir"
echo "ok $CASE"

CASE="a failed mkfifo"
run_remote 1 STUB_MKFIFO_FAIL=1
[[ -s $MKFIFO_LOG ]] || fail "the run ended before it reached mkfifo: $(<"$LOG")"
assert_no_session_left
echo "ok $CASE"

# A run killed outright leaves its session dir, and earlier versions of the script kept it under the results directory.
CASE="a killed run's pipes are swept"
mkdir -p "$SCRATCH/remote-session-1-2-3" "$REPO/target/mutants/remote-session-4-5-6"
"$REAL_MKFIFO" "$SCRATCH/remote-session-1-2-3/control-in" "$REPO/target/mutants/remote-session-4-5-6/control-in"
run_remote 0
assert_session_under "$SCRATCH"
echo "ok $CASE"

CASE="a stale session that cannot be removed stops the run"
if [ "$(id -u)" -eq 0 ]; then
  echo "skip $CASE: root can delete from an unwritable dir"
else
  mkdir -p "$SCRATCH/remote-session-7-8-9"
  "$REAL_MKFIFO" "$SCRATCH/remote-session-7-8-9/control-in"
  chmod 500 "$SCRATCH/remote-session-7-8-9"
  run_remote 74
  [[ "$(<"$LOG")" == *"could not remove the stale session $SCRATCH/remote-session-7-8-9"* ]] || fail "the run did not name the session it could not remove: $(<"$LOG")"
  [[ ! -e $MKFIFO_LOG ]] || fail "the run went on to make a session: $(<"$MKFIFO_LOG")"
  chmod 700 "$SCRATCH/remote-session-7-8-9"
  echo "ok $CASE"
fi

echo "mutants-remote: ok"
