#!/usr/bin/env bash
#
# Mutation-test only the lines this commit changes.
#
# A passing test suite proves the tests run; it does not prove they would notice
# if the code were wrong. cargo-mutants perturbs the implementation and reports
# mutations no test catches - a surviving mutant IS a non-discriminating test.
#
# Scoped with --in-diff rather than running the whole tree, so the cost is
# proportional to the change. A full sweep runs in CI.

set -euo pipefail

# Under target/ because it is already gitignored, and overwritten each run so
# nothing needs cleaning up.
DIFF_DIR="target/mutants"
DIFF="$DIFF_DIR/staged.diff"
LOG="$DIFF_DIR/run.log"
mkdir -p "$DIFF_DIR"

# pre-commit stashes unstaged changes before running hooks, so the working tree
# matches the index here - which is what makes diffing the index correct.
git diff --cached -- '*.rs' > "$DIFF"

if [ ! -s "$DIFF" ]; then
  echo "no staged Rust changes; nothing to mutate"
  exit 0
fi

# --minimum-test-timeout: cargo-mutants derives the per-mutant timeout from the
# unmutated baseline, which on a fast suite is a second or two. With -j running
# several full suites at once on a loaded machine, a healthy mutant can exceed
# that and be recorded as TIMEOUT. Give it real headroom so a timeout means what
# it should.
#
# A timeout is NOT a failure. Some mutations produce an infinite loop (swapping
# `-=` for `/=` yields `x /= 1`, a no-op), and a suite that hangs has detected
# the mutant just as surely as one that fails. cargo-mutants exits 0 for these,
# which is correct.
exec cargo mutants --in-diff "$DIFF" -j 4 --no-shuffle --minimum-test-timeout 60
