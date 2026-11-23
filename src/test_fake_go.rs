//! A stub `go` for tests of `go vet`'s package-shaped invocation, which must not depend on Go being installed.

use std::path::{Path, PathBuf};

use crate::languages::definitions;
use crate::languages::spec::ToolSpec;

/// The registry's `go vet`, resolving the stub [`workspace`] writes rather than whatever `go` is on PATH. Derived rather than copied, so a change to the real spec reaches these tests.
pub(crate) fn spec() -> ToolSpec {
    ToolSpec {
        command: &["fakego", "vet"],
        local_paths: &["fakego"],
        config_files: &["marker"],
        ..definitions::GO_VET.clone()
    }
}

/// What the stub's `vet` call prints and how it exits.
#[derive(Default)]
pub(crate) struct Vet {
    /// What `go vet` writes to stderr, where it keeps every diagnostic.
    pub(crate) stderr: &'static str,
    pub(crate) exit: i32,
}

/// A Go workspace at `root` configured for [`spec`]: each of `files` empty with its directories, the marker, and the stub `fakego`. Returns the log its `vet` calls append their arguments to.
///
/// `fakego list` answers as `go list -e -json` would, from the files on disk. Every `*.go` file in a directory is in its package except that a `*_ignored.go` file stands for one build constraints exclude, and a `*_invalid.go` file for one with a malformed `//go:build` line, which go reports in `InvalidGoFiles` with a position-less package error. A directory whose name starts with `broken` has an unresolvable import, and one named `outside` comes back with an error and no `Dir`, as a module go.work does not use does. One named `outside_escape` comes back the same way with the message Go 1.25.4 prints for such a directory whose name holds an OSC 52 clipboard write: the name goes into `Err` unquoted, JSON-escaped only, so it decodes to raw ESC and BEL, and the message carries a newline and a tab of its own. Any other call appends its arguments to the log one per line, refuses `.go` files from more than one directory as `go vet` does, and otherwise prints and exits as `vet` says.
pub(crate) fn workspace(root: &Path, files: &[&str], vet: Vet) -> PathBuf {
    for file in files {
        let path = root.join(file);
        std::fs::create_dir_all(path.parent().expect("a file has a parent")).expect("mkdir");
        std::fs::write(path, "").expect("write");
    }
    std::fs::write(root.join("marker"), "").expect("marker");
    let log = root.join("argv.log");
    let script = SCRIPT
        .replace("@LOG@", &log.to_string_lossy())
        .replace("@STDERR@", vet.stderr)
        .replace("@EXIT@", &vet.exit.to_string());
    crate::test_support::write_executable(&root.join("fakego"), script);
    log
}

const SCRIPT: &str = r#"#!/bin/sh
if [ "$1" = list ]; then
  shift
  for d in "$@"; do
    case "$d" in -*) continue ;; esac
    case "$d" in */outside_escape)
      printf '%s\n' '{"Error":{"ImportStack":[],"Pos":"","Err":"directory outside\u001b]52;c;cHduZWQ=\u0007 is contained in a module that is not one of the workspace modules listed in go.work. You can add the module to the workspace using:\n\tgo work use ."}}'
      continue ;;
    esac
    case "$d" in */outside)
      printf '{"Error":{"Err":"directory outside is contained in a module that is not one of the workspace modules"}}\n'
      continue ;;
    esac
    gofiles= ; gs= ; invalid= ; is=
    for f in "$d"/*.go; do
      [ -e "$f" ] || continue
      n="${f##*/}"
      case "$n" in
        *_ignored.go) ;;
        *_invalid.go) invalid="$invalid$is\"$n\""; is=, ;;
        *) gofiles="$gofiles$gs\"$n\""; gs=, ;;
      esac
    done
    printf '{"Dir":"%s","GoFiles":[%s],"InvalidGoFiles":[%s]' "$(cd "$d" && pwd -P)" "$gofiles" "$invalid"
    if [ -n "$invalid" ]; then
      printf ',"Error":{"Pos":"","Err":"parsing //go:build line: unexpected end of expression"}'
    fi
    case "$d" in */broken*)
      printf ',"DepsErrors":[{"Pos":"broken/b.go:3:8","Err":"package ex/nope is not in std"}]' ;;
    esac
    printf '}\n'
  done
  exit 0
fi
for a in "$@"; do printf '%s\n' "$a" >> '@LOG@'; done
# Real go refuses `.go` files from more than one directory, which is how 3.1.4 failed a diff spanning packages.
first=
for a in "$@"; do
  case "$a" in *.go)
    d=$(dirname "$a")
    if [ -z "$first" ]; then first="$d"
    elif [ "$d" != "$first" ]; then
      echo "named files must all be in one directory; have $first and $d" >&2
      exit 1
    fi ;;
  esac
done
cat >&2 <<'ERR'
@STDERR@
ERR
exit @EXIT@
"#;
