#!/usr/bin/env bash
# Source after HOST and AI1_CI_ROLE are assigned. Legacy explicit host overrides
# retain their transport; ai-1 always executes inside the installed CI sandbox.
ssh() {
  local options=() target command_text
  while [ "$#" -gt 0 ]; do
    case "$1" in
      -o)
        if [ "$#" -lt 2 ]; then
          printf 'ai-1 transport: -o requires a value\n' >&2
          return 2
        fi
        options+=("$1" "$2"); shift 2 ;;
      -*) printf 'ai-1 transport: unsupported SSH option %s\n' "$1" >&2; return 2 ;;
      *) break ;;
    esac
  done
  if [ "$#" -eq 0 ]; then
    printf 'ai-1 transport: destination required\n' >&2
    return 2
  fi
  case "$AI1_CI_ROLE" in drep-mutants|tattood-mutants) ;; *) return 2 ;; esac
  target="$1"
  shift
  if [ "$target" != "steve@192.168.68.88" ] || [ "$#" -eq 0 ]; then
    command ssh "${options[@]}" "$target" "$@"
    return
  fi
  if [ "$#" -eq 1 ]; then
    command_text="$1"
  else
    printf -v command_text '%q ' "$@"
  fi
  printf -v command_text 'sudo /usr/local/lib/ai-ci/offload.py %q %q' "$AI1_CI_ROLE" "$command_text"
  command ssh "${options[@]}" "$target" "$command_text"
}

rsync() {
  local argument ai1_transfer=0 rsync_path
  for argument in "$@"; do
    case "$argument" in
      steve@192.168.68.88:*) ai1_transfer=1 ;;
      --rsync-path|--rsync-path=*)
        printf 'ai-1 transport: caller may not replace the remote execution path\n' >&2
        return 2 ;;
    esac
  done
  if [ "$ai1_transfer" -eq 1 ]; then
    case "$AI1_CI_ROLE" in drep-mutants|tattood-mutants) ;; *) return 2 ;; esac
    printf -v rsync_path 'sudo /usr/local/lib/ai-ci/offload.py %q rsync' "$AI1_CI_ROLE"
    command rsync --rsync-path="$rsync_path" "$@"
  else
    command rsync "$@"
  fi
}
