#!/usr/bin/env bash
#
# Mutation-test only the lines this commit changes.
#
# A passing test suite proves the tests run; it does not prove they would notice
# if the code were wrong. cargo-mutants perturbs the implementation and reports
# mutations no test catches - a surviving mutant IS a non-discriminating test.
#
# Scoped with --in-diff rather than running the whole tree, so the cost is
# proportional to the change. CI runs the full sweep through the same
# scripts/mutants-run.sh, which is where the pass/fail rule lives.

set -euo pipefail

DIFF_DIR="target/mutants"
DIFF="$DIFF_DIR/staged.diff"
mkdir -p "$DIFF_DIR"

# pre-commit stashes unstaged changes before running hooks, so the working tree
# matches the index here - which is what makes diffing the index correct.
git diff --cached -- '*.rs' > "$DIFF"

if [ ! -s "$DIFF" ]; then
  echo "no staged Rust changes; nothing to mutate"
  exit 0
fi

# Through mutants-remote.sh, which offloads the run to a bigger machine and
# falls back to a local run when it cannot be reached. The verdict is
# scripts/mutants-run.sh's either way.
exec ./scripts/mutants-remote.sh --in-diff "$DIFF"
