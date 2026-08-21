//! `drep init`'s git-hook installer.
//!
//! This is the only part of `drep init` that can damage something. It writes
//! into `.git/hooks/`, which the user owns, so every branch below is
//! deliberate: a foreign hook is left alone, a chainer is rewritten only when
//! it does not already chain, and `core.hooksPath` is resolved the way git
//! resolves it (relative to the *repository*, not the cwd).
//!
//! Two hooks are installed today: `pre-commit` (one `drep check --staged`)
//! and `pre-push` (one `drep check --push-gate --diff <remote-oid>` per ref on
//! stdin).
//! Both begin with `# Managed by \`drep init\`.` - that marker is how this
//! module recognises a hook it wrote and may rewrite.

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};

use crate::diff;

/// The marker every drep-managed hook and chainer begins with.
///
/// Every body below is built with `concat!`/`format!` around this constant
/// rather than repeating the literal, so a rename really does update
/// everywhere. It did not: the three bodies each hardcoded the string, which
/// meant a rename would leave `is_drep_managed` unable to recognise the hooks
/// drep had just written - and drep would then refuse to update its own hook.
/// The marker text as a *literal*, so `concat!` can build the hook bodies
/// from it. `concat!` accepts only literals, not consts, which is why this is
/// a macro rather than a plain `const` alone; [`MANAGED_MARKER`] is the value
/// every non-literal caller uses.
macro_rules! managed_marker {
    () => {
        "# Managed by `drep init`."
    };
}

pub const MANAGED_MARKER: &str = managed_marker!();

/// The body drep writes for `pre-commit`.
///
/// Two commands, in this order. `lint-docs` is rule-based and takes ~10 ms, so
/// an obvious documentation defect does not cost an LLM round trip; `check`
/// sends the staged code to a model and is the expensive half.
///
/// `--fail-on error` rather than `--strict`: under the severity scale the doc
/// checks use, `--strict` blocks on *any* finding, which over a real
/// repository is dominated by line length and trailing whitespace. Measured on
/// drep's own tree that is 75 findings, none above `info`. A hook that blocks
/// a commit over a long line is a hook that gets deleted. `error` is one
/// check - an unclosed fence, which renders the rest of the document as code.
///
/// `exec` on the last command is what keeps the LLM client's exit status,
/// which is what aborts the commit when a finding gates it. The first command
/// cannot be `exec`ed, so its status is propagated explicitly.
pub const PRE_COMMIT_BODY: &str = concat!(
    "#!/bin/sh\n",
    managed_marker!(),
    r##"
# Runs the linters this repo configures, and an LLM review of the staged code.
if ! command -v drep > /dev/null 2>&1; then
    echo "drep: not found on PATH; refusing to let the commit through unreviewed." >&2
    exit 1
fi
drep lint-docs --staged --fail-on error || exit $?
exec drep check --staged
"##
);

/// The body drep writes for `pre-push`.
///
/// git sends one line per ref on stdin:
///   `<local ref> <local oid> <remote ref> <remote oid>`
/// An all-zero remote oid means the branch does not exist upstream yet, so
/// there is no previous state to diff against; fall back to the remote's
/// default branch. An all-zero *local* oid is a branch deletion, which has
/// no content to review.
///
/// `--push-gate` performs a cache-only verdict first. A cold review is
/// completed and cached, but exits 3 so Git closes the connection it opened
/// before invoking this hook; repeating the push reconnects and reads the
/// warm verdict instead of resuming a transport that sat idle for minutes.
pub const PRE_PUSH_BODY: &str = concat!(
    "#!/bin/sh\n",
    managed_marker!(),
    r##"
# git runs this as: pre-push <remote-name> <remote-url>, and sends one line per
# ref on stdin:
#   <local ref> <local oid> <remote ref> <remote oid>
#
# Three things here are not obvious, and each was a real defect:
#
#  * The ref being pushed is NOT always the checked-out branch
#    (`git push origin feature:feature` from elsewhere, or `git push --all`),
#    so `--tip` names the oid actually being pushed. Reviewing HEAD instead
#    lets the pushed code through unseen.
#  * The base search is BOUNDED. An all-zero remote oid means the branch is
#    new upstream; falling back to the root commit there sends the repository's
#    entire history to the model, which on a mature repo is hours of wall clock
#    and real money from one `git push`.
#  * `drep` reads no stdin, but `< /dev/null` makes that structural: a command
#    inside a `while read` loop that did would swallow the remaining refs and
#    the push would go green having reviewed one of them.
remote="${1:-origin}"
zeros=0000000000000000000000000000000000000000
status=0

if ! command -v drep > /dev/null 2>&1; then
    echo "drep: not found on PATH; refusing to let the push through unreviewed." >&2
    echo "  (GUI git clients often use a minimal PATH - see the drep README.)" >&2
    exit 1
fi

while read -r _local_ref local_oid _remote_ref remote_oid; do
    # A branch deletion has no content to review.
    case "$local_oid" in "$zeros"*) continue ;; esac

    case "$remote_oid" in
        "$zeros"*)
            # New upstream: find the nearest sensible base, cheapest first, and
            # never scan further back than 50 commits.
            base=$(git rev-parse --verify --quiet "$remote/HEAD") ||
            base=$(git rev-parse --verify --quiet "$remote/main") ||
            base=$(git rev-parse --verify --quiet "$remote/master") ||
            base=$(git rev-parse --verify --quiet "$local_oid~50") ||
            base=$(git rev-list --max-parents=0 "$local_oid" | tail -n 1)
            ;;
        *) base=$remote_oid ;;
    esac

    [ -n "$base" ] || continue

    drep check --push-gate --diff "$base" --tip "$local_oid" < /dev/null
    rc=$?
    # Failure precedence is semantic, not numeric: 2 (could not analyze), then
    # 1 (findings), then 3 (review cached; reconnect), then 0. Exit 3 is
    # numerically highest but is a successful review, so it must not hide a
    # harder failure from another ref.
    case "$rc" in
        2) status=2 ;;
        1) [ "$status" -ne 2 ] && status=1 ;;
        3) [ "$status" -eq 0 ] && status=3 ;;
    esac
done

exit $status
"##
);

/// The chainer body, parameterised on the hook name.
///
/// This is what goes in the `core.hooksPath` directory: an `exec` shim that
/// forwards to the repo-local hook git would otherwise ignore entirely. With
/// `core.hooksPath` set, git does not look in `.git/hooks` at all, so without
/// a chainer a perfectly good repo-local hook simply never runs.
///
/// The body names no repository, so a chainer written into a shared directory
/// is safe for every repo that uses it: it forwards when a repo-local hook
/// exists and falls through silently when one does not.
pub fn chainer_body(name: &str) -> String {
    format!(
        "\
#!/bin/sh
{MANAGED_MARKER}
# Chains to the repo-local {name} hook, which git ignores while core.hooksPath
# is set. `exec` matters twice: it keeps the local hook's exit status (that is
# what aborts the operation) and hands over stdin unread, which is how git
# delivers the refs being pushed.
LOCAL_HOOK=\"$(git rev-parse --git-common-dir)/hooks/{name}\"
if [ -x \"$LOCAL_HOOK\" ]; then
    exec \"$LOCAL_HOOK\" \"$@\"
fi
"
    )
}

/// Which git hook to install.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum HookKind {
    /// `git push` triggers `drep check --diff <remote-oid>`. The default.
    PrePush,
    /// `git commit` triggers `drep check --staged`.
    PreCommit,
    /// Both `pre-commit` and `pre-push`.
    Both,
    /// Neither. `drep init` writes `drep.toml` and skips the hooks.
    None,
}

/// The names `kind` installs.
///
/// `Both` yields `["pre-commit", "pre-push"]` in that order, so `pre-commit`
/// is installed before `pre-push` and a failure in one does not change the
/// other.
pub fn hook_names(kind: HookKind) -> &'static [&'static str] {
    match kind {
        HookKind::None => &[],
        HookKind::PrePush => &["pre-push"],
        HookKind::PreCommit => &["pre-commit"],
        HookKind::Both => &["pre-commit", "pre-push"],
    }
}

/// The body drep writes for `name`. `None` for an unknown name.
pub fn hook_body(name: &str) -> Option<&'static str> {
    match name {
        "pre-commit" => Some(PRE_COMMIT_BODY),
        "pre-push" => Some(PRE_PUSH_BODY),
        _ => None,
    }
}

/// `core.hooksPath`, resolved the way git resolves it: an absolute value is
/// used as-is, a relative one is relative to the *repository*, not the cwd.
///
/// Resolving against the cwd would write a chainer into whatever directory
/// the caller happened to be in.
pub fn resolve_hooks_dir(root: &Path, value: &str) -> PathBuf {
    let candidate = Path::new(value);
    if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        root.join(value)
    }
}

/// True when `body` is a hook drep wrote and may therefore rewrite.
pub fn is_drep_managed(body: &str) -> bool {
    let mut lines = body.lines();
    match lines.next() {
        Some(first) if first == MANAGED_MARKER => true,
        Some(first) if first.starts_with("#!") => lines.next() == Some(MANAGED_MARKER),
        _ => false,
    }
}

/// Install the hooks. Writes to `out`; never panics.
pub async fn install<W: Write>(
    out: &mut W,
    root: &Path,
    kind: HookKind,
    force: bool,
) -> Result<()> {
    let names = hook_names(kind);
    // `--hooks none` must not create directories or ask git anything. It is
    // the escape hatch for "write me a config, leave my repo alone", and an
    // escape hatch with side effects is not one.
    if names.is_empty() {
        return Ok(());
    }
    let hooks_dir = locate_hooks_dir(root).await?;

    // Resolve every git-owned destination before writing anything. If git
    // cannot expand core.hooksPath, returning an error after installing the
    // repo-local hooks would leave a partial installation that still never
    // runs while claiming the operation failed.
    let configured = run_git_config_path(root).await?;
    std::fs::create_dir_all(&hooks_dir)
        .with_context(|| format!("could not create {}", hooks_dir.display()))?;

    for name in names {
        // Total rather than `expect`: `hook_names` and `hook_body` are two
        // matches over the same vocabulary, and a future hook added to one and
        // not the other must not panic inside an installer the user is
        // trusting with their `.git` directory.
        let body =
            hook_body(name).ok_or_else(|| anyhow!("no hook body is defined for `{name}`"))?;
        let path = hooks_dir.join(name);
        match std::fs::read_to_string(&path) {
            Ok(existing) => {
                if is_drep_managed(&existing) {
                    write_executable(&path, body)?;
                    writeln!(out, "  Wrote {}", path.display())?;
                } else if force {
                    // `--force` is one flag serving two destinations, and
                    // `config_file::write` is what tells the user to reach for
                    // it. Keeping a copy makes replacement recoverable.
                    let backup = path.with_extension("drep-backup");
                    std::fs::write(&backup, &existing)
                        .with_context(|| format!("could not back up to {}", backup.display()))?;
                    write_executable(&path, body)?;
                    writeln!(out, "  Wrote {}", path.display())?;
                    writeln!(out, "  Your previous hook is saved at {}", backup.display())?;
                } else {
                    writeln!(
                        out,
                        "  {} already exists and was not written by drep; leaving it alone.",
                        path.display()
                    )?;
                    writeln!(out, "  Re-run with --force to replace it.")?;
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                write_executable(&path, body)?;
                writeln!(out, "  Wrote {}", path.display())?;
            }
            Err(err) => {
                return Err(anyhow::Error::new(err)
                    .context(format!("could not read existing hook {}", path.display())));
            }
        }
    }

    // core.hooksPath chainer: query with --type=path so git expands ~ and
    // ~user itself; hand-rolled expansion mangled `~alice/hooks`, and $HOME
    // is unset in some environments.
    if let Some(value) = configured {
        writeln!(out, "  core.hooksPath is set to {value}")?;
        writeln!(
            out,
            "  git looks there and not in .git/hooks, so a repo hook needs a chainer."
        )?;

        let chainer_dir = resolve_hooks_dir(root, &value);
        for name in names {
            ensure_chainer(out, &chainer_dir, name)?;
        }
    }

    Ok(())
}

/// Resolve the hooks directory git would consult for repo-local hooks.
///
/// `git rev-parse --git-common-dir` rather than `root/.git`: in a linked
/// worktree or a submodule `.git` is a *file*, so the literal path does not
/// exist and the hook silently never runs.
async fn locate_hooks_dir(root: &Path) -> Result<PathBuf> {
    let common = diff::run_git(root, &["rev-parse", "--git-common-dir"])
        .await
        .with_context(|| format!("could not locate git common dir under {}", root.display()))?;
    let common = PathBuf::from(common);
    let hooks_dir = if common.is_absolute() {
        common
    } else {
        root.join(common)
    };
    Ok(hooks_dir.join("hooks"))
}

/// Query `core.hooksPath`. `Ok(None)` when genuinely unset.
///
/// `git config --get` exits **1** for "not found" and >=2 for a real error, so
/// the two are distinguishable and must be distinguished: swallowing an error
/// as "unset" means skipping the chainer while `core.hooksPath` is in fact set,
/// which leaves the hook drep just wrote unable to ever run - reported as
/// success.
///
/// An empty value is "unset" for our purposes and is *not* an error: git reads
/// a blank `core.hooksPath` back as present-but-empty, which disables hooks
/// entirely rather than naming a directory.
async fn run_git_config_path(root: &Path) -> Result<Option<String>> {
    // An *empty* value reads back as present-but-blank, which drep treats as
    // unset, so it collapses into the same `None` as "no such key".
    match diff::git_query(root, &["config", "--get", "--type=path", "core.hooksPath"]).await {
        Ok(Some(value)) if value.is_empty() => Ok(None),
        Ok(value) => Ok(value),
        Err(err) => Err(anyhow!(
            "could not read core.hooksPath ({err}); refusing to install a hook that \
             may never run"
        )),
    }
}

/// Make sure a chainer for `name` exists in `dir`, executable, and chains.
///
/// Leaves a foreign chainer alone, reports the situation. `git` ignores a
/// non-executable hook silently, which is the entire reason this branch
/// exists.
fn ensure_chainer<W: Write>(out: &mut W, dir: &Path, name: &str) -> Result<()> {
    let chainer = dir.join(name);
    match std::fs::read_to_string(&chainer) {
        Ok(body) if is_drep_managed(&body) => {
            let current = chainer_body(name);
            if body != current {
                write_executable(&chainer, &current)?;
                writeln!(out, "  Wrote {}", chainer.display())?;
                return Ok(());
            }
            ensure_executable(out, &chainer)?;
            return Ok(());
        }
        Ok(body) => {
            let marker = format!("hooks/{name}");
            let mentions_hook_in_command = body.lines().any(|line| {
                let line = line.trim_start();
                !line.starts_with('#') && line.contains(&marker)
            });
            if mentions_hook_in_command {
                ensure_executable(out, &chainer)?;
                return Ok(());
            }
            writeln!(
                out,
                "  {} exists but does not appear to chain to the repo-local hook.",
                chainer.display()
            )?;
            writeln!(out, "  drep will not run until it does.")?;
            return Ok(());
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => {
            return Err(anyhow::Error::new(err).context(format!(
                "could not read existing chainer {}",
                chainer.display()
            )));
        }
    }

    std::fs::create_dir_all(dir)
        .with_context(|| format!("could not create chainer dir {}", dir.display()))?;
    write_executable(&chainer, &chainer_body(name))?;
    // Named in full, and flagged as outside the repository: this is the one
    // thing `drep init` writes that is not under `root`, and a shared hooks
    // directory is shared with every other repo on the machine.
    writeln!(
        out,
        "  Wrote a chainer at {} (outside this repository)",
        chainer.display()
    )?;
    Ok(())
}

/// Make an existing forwarding hook executable, reporting only a repair.
fn ensure_executable<W: Write>(out: &mut W, path: &Path) -> Result<()> {
    let was_executable = crate::languages::runner::is_executable(path);
    set_executable(path)?;
    if !was_executable {
        writeln!(out, "  {} is not executable; making it so.", path.display())?;
    }
    Ok(())
}

/// Write `body` to `path` and make it executable, atomically.
///
/// Via a sibling temp file and a rename, because `fs::write` truncates in
/// place: an interruption mid-write leaves a *truncated but executable* hook,
/// and since these bodies open with a shebang and comments, a truncated one
/// exits 0 and waves every push through. A rename is atomic on the same
/// filesystem, so a hook is either the old one or the new one.
fn write_executable(path: &Path, body: &str) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("hook path {} has no parent", path.display()))?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)
        .with_context(|| format!("could not write hook {}", path.display()))?;
    temporary
        .write_all(body.as_bytes())
        .with_context(|| format!("could not write hook {}", path.display()))?;
    set_executable(temporary.path())?;
    temporary
        .as_file()
        .sync_all()
        .with_context(|| format!("could not write hook {}", path.display()))?;
    temporary.persist(path).map_err(|err| {
        anyhow::Error::new(err.error).context(format!("could not install hook {}", path.display()))
    })?;
    Ok(())
}

/// Make `path` executable. A no-op where the platform has no such bit.
///
/// One function with the `cfg` inside its body, not two cfg-gated
/// definitions - the same rule `languages::runner::is_executable` follows and
/// for the same reason: the inactive definition is unreachable on this
/// platform, so every mutation of it survives by construction and shows up in
/// `cargo mutants` as a finding no test can ever address.
fn set_executable(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(path)
            .with_context(|| format!("could not stat {}", path.display()))?
            .permissions();
        // `| 0o111`, not `| 0o755`: adding the execute bits is the whole
        // requirement, and OR-ing 0o755 onto a deliberately-private 0o600 file
        // grants group and other read access to a file in what may be a shared
        // hooks directory.
        perms.set_mode(perms.mode() | 0o111);
        std::fs::set_permissions(path, perms)
            .with_context(|| format!("could not chmod {}", path.display()))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The three bodies are built from `managed_marker!`, so a rename really
    /// does reach all of them.
    ///
    /// The bodies used to hardcode the marker text while the constant's own
    /// doc claimed they referenced it. A rename would then have left
    /// `is_drep_managed` unable to recognise a hook drep had just written -
    /// so drep would refuse to update its own hook, and the constant would
    /// have been documentation of an invariant it did not hold.
    #[test]
    fn every_body_is_built_from_the_marker_constant() {
        for body in [
            PRE_COMMIT_BODY.to_owned(),
            PRE_PUSH_BODY.to_owned(),
            chainer_body("pre-push"),
        ] {
            assert!(
                body.contains(MANAGED_MARKER),
                "body must carry the marker: {body}"
            );
            assert!(
                is_drep_managed(&body),
                "and must therefore be recognised as drep's own"
            );
        }
        assert!(!is_drep_managed("#!/bin/sh\necho hi\n"));
    }

    /// The two hook bodies are distinct and each runs the mode it is for.
    ///
    /// Nothing pinned this: `"pre-commit" => Some(PRE_PUSH_BODY)` passed the
    /// whole suite, because the tests compared installed bytes against
    /// `hook_body(name)` - the implementation itself - and only pre-push was
    /// ever executed.
    #[test]
    fn each_hook_body_runs_the_mode_it_is_named_for() {
        let pre_commit = hook_body("pre-commit").expect("known");
        let pre_push = hook_body("pre-push").expect("known");

        assert!(
            pre_commit.contains("drep check --staged"),
            "pre-commit reviews what is staged: {pre_commit}"
        );
        assert!(
            !pre_commit.contains("--diff"),
            "and not a diff against a ref: {pre_commit}"
        );
        assert!(
            pre_push.contains("drep check --push-gate --diff") && pre_push.contains("--tip"),
            "pre-push reviews a range ending at the pushed ref: {pre_push}"
        );
        assert!(
            !pre_push.contains("--staged"),
            "nothing is staged at push time: {pre_push}"
        );
        assert_ne!(pre_commit, pre_push);
        assert!(hook_body("unknown-hook").is_none());
    }
}
