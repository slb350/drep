#!/usr/bin/env bash
#
# The one definition of where mutation results live, sourced by every script in
# this trio.
#
# mutants-run.sh reads `missed.txt` out of this directory to reach its verdict,
# mutants-staged.sh writes the staged diff into it, and mutants-remote.sh
# mirrors it back from the remote host. It was the same string literal in six
# places across three files, all of them silently wrong the day one of them
# changed: a stale copy does not error, it just stops finding missed.txt.
#
# target/ because it is already gitignored. Overridable so a caller with a
# different layout does not have to edit three scripts.
MUTANTS_OUT_DIR="${MUTANTS_OUT_DIR:-target/mutants}"

# One lock wait policy for both the remote transaction wrapper and direct/CI
# mutation runs. The two scripts acquire the lock at different boundaries, but
# accepting and defaulting the operator's value must not drift between them.
MUTANTS_HOST_LOCK_WAIT_SECONDS="${DREP_MUTANTS_HOST_LOCK_WAIT_SECONDS:-1800}"

validate_mutants_host_lock_wait_seconds() {
  local caller="$1"
  case "$MUTANTS_HOST_LOCK_WAIT_SECONDS" in
    ''|*[!0-9]*)
      echo "$caller: DREP_MUTANTS_HOST_LOCK_WAIT_SECONDS must be a non-negative integer" >&2
      return 64
      ;;
  esac
}

# Remove one file or tree. `find` does not follow a symlink given as its root,
# and an entry that vanishes mid-walk is not a failure.
remove_tree() {
  find "$1" -depth -delete 2>/dev/null || true
}

# A repo-relative path, or one below a remote home: never absolute and never
# stepping outside through a . or .. component.
is_contained_path() {
  case "$1" in
  '' | /* | . | .. | ./* | ../* | */. | */.. | */./* | */../*) return 1 ;;
  esac
}

# One mutation run per checkout at a time. The hook, a local run and a remote
# transaction all write $MUTANTS_OUT_DIR and this checkout's scratch, so a
# second run waits instead of deleting the first run's results or copies. macOS
# has no flock, so the lock is a directory made atomically by mkdir that holds
# its owner's PID, and a lock whose owner is gone is taken over. A script called
# by one already holding the lock inherits it through the environment.
MUTANTS_CHECKOUT_LOCK="${MUTANTS_OUT_DIR}.lock"

acquire_checkout_lock() {
  local caller="$1" owner waited=0
  [ -z "${MUTANTS_CHECKOUT_LOCK_HELD:-}" ] || return 0
  validate_mutants_host_lock_wait_seconds "$caller" || return
  mkdir -p "$(dirname "$MUTANTS_CHECKOUT_LOCK")"
  until mkdir "$MUTANTS_CHECKOUT_LOCK" 2>/dev/null; do
    owner="$(cat "$MUTANTS_CHECKOUT_LOCK/pid" 2>/dev/null || true)"
    # An owner that died, or one that died between mkdir and writing its PID
    # (an empty lock older than a minute), no longer holds anything.
    if { [ -n "$owner" ] && ! kill -0 "$owner" 2>/dev/null; } ||
      { [ -z "$owner" ] && [ -n "$(find "$MUTANTS_CHECKOUT_LOCK" -maxdepth 0 -mmin +1 2>/dev/null)" ]; }; then
      remove_tree "$MUTANTS_CHECKOUT_LOCK"
      continue
    fi
    if [ "$waited" -ge "$MUTANTS_HOST_LOCK_WAIT_SECONDS" ]; then
      echo "$caller: another mutation run in this checkout holds $MUTANTS_CHECKOUT_LOCK" >&2
      return 75
    fi
    sleep 1
    waited=$((waited + 1))
  done
  printf '%s\n' "$$" >"$MUTANTS_CHECKOUT_LOCK/pid"
  MUTANTS_CHECKOUT_LOCK_OWNER=1
  export MUTANTS_CHECKOUT_LOCK_HELD=1
}

release_checkout_lock() {
  [ -z "${MUTANTS_CHECKOUT_LOCK_OWNER:-}" ] || remove_tree "$MUTANTS_CHECKOUT_LOCK"
}
