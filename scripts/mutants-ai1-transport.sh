#!/usr/bin/env bash
# Source after HOST and AI1_CI_ROLE are assigned. Legacy explicit host overrides
# retain their transport; ai-1 always executes inside the installed CI sandbox.
ssh() {
  local options=() target command_text
  while [ "$#" -gt 0 ]; do
    case "$1" in
      -o) options+=("$1" "$2"); shift 2 ;;
      *) break ;;
    esac
  done
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
  if [ "$HOST" = "steve@192.168.68.88" ]; then
    command rsync --rsync-path="sudo /usr/local/lib/ai-ci/offload.py $AI1_CI_ROLE rsync" "$@"
  else
    command rsync "$@"
  fi
}
