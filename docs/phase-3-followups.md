# Phase 3 follow-ups — from drep 1.x reviewing drep 2.0

**Status: the five "must fix" items and both of my weak tests were fixed in
`8a20522`.** What remains below is the deferred list, kept because the reasoning
for deferring is worth having when someone hits one of them.

The pre-push gate analyzed the Phase 3a code and returned 23 advisory findings.
Triaged below. **Verified** means I reproduced it; **plausible** means it reads
correct but I have not confirmed it.

## Fixed in 8a20522

Multi-line fenced JSON (`FENCE_RE` had no `(?s)`), the trailing-comma repair
corrupting string contents, repairs not composing, `LlmClient`'s derived `Debug`
leaking the API key, and `run_git` lacking a timeout at three of its four call
sites. Plus two tests of mine that did not discriminate — one accepting
`"max_tokens": null` where the contract said absent, one using `Error::api`
instead of `Error::api_status`.

## Still open — deferred deliberately

**`diff/mod.rs:169` — a `git_ref` beginning with `--` is parsed by git as an
option.** `drep check --diff --output=/tmp/x` reaches git as a flag. Fix: pass
`--` before the revision, or reject refs starting with `-`. Low risk today
because the ref comes from a hook script, not a web form, but it is a one-line
fix worth doing in Phase 5 when the CLI is wired up.

**`runner/mod.rs:139` — `which_first` checks `is_file()`, not executability.**
Contradicts the repo-local path check, so `tool_status` can report `Ok` for a
PATH entry `run_tool` then cannot execute. Reuse the `is_executable` helper that
already exists a few lines above.

## Judged not worth acting on now

- Non-UTF-8 git paths / `core.quotePath` (`diff/mod.rs:110`): real, but drep
  targets source files whose paths are UTF-8 in every repo we support.
- SHA-256 repositories and the hardcoded `EMPTY_TREE` (`diff/mod.rs:33`): real
  and worth a note, but no such repo is in scope yet.
- Windows `PATHEXT` in `which_first`: 2.0 ships unix-first; revisit at Phase 7 if
  Windows targets are added.
- `wait_with_output` buffering unbounded tool output: the 120s timeout bounds it
  in practice.
- TOML error line/column, the `user_content` clone, `content.to_string()` on the
  unfenced path: cosmetic.

## Note on the push failure

Exit 1 came from pre-commit's "files were modified by this hook", not from the
findings — all 23 were advisory. `Cargo.toml`/`Cargo.lock` were dirty because
blake3/directories were added for 3b after 3a was committed. Commit those with
3b and re-push.
