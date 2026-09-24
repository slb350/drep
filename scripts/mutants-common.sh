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
# second run waits instead of deleting the first run's results or copies. The
# lock is a kernel flock on the file below, held on this shell's descriptor 6:
# the kernel drops it when the last process holding that descriptor exits, so a
# killed run never leaves one behind. perl takes it because macOS has no
# flock(1). A script called by one already holding the lock inherits it through
# the environment instead of waiting on itself.
MUTANTS_CHECKOUT_LOCK="${MUTANTS_OUT_DIR}.lock"

acquire_checkout_lock() {
  local caller="$1"
  [ -z "${MUTANTS_CHECKOUT_LOCK_HELD:-}" ] || return 0
  validate_mutants_host_lock_wait_seconds "$caller" || return
  mkdir -p "$(dirname "$MUTANTS_CHECKOUT_LOCK")"
  exec 6>>"$MUTANTS_CHECKOUT_LOCK"
  # shellcheck disable=SC2016  # perl source, not shell.
  if ! perl -MFcntl=:flock -e '
    open(my $lock, ">&=", 6) or exit 2;
    my $deadline = time + $ARGV[0];
    until (flock($lock, LOCK_EX | LOCK_NB)) { exit 1 if time >= $deadline; sleep 1 }
  ' "$MUTANTS_HOST_LOCK_WAIT_SECONDS"; then
    echo "$caller: another mutation run in this checkout holds $MUTANTS_CHECKOUT_LOCK" >&2
    return 75
  fi
  export MUTANTS_CHECKOUT_LOCK_HELD=1
}
