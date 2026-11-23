#!/usr/bin/env bash
# The only way the mutation scripts reach homelab-1, where mutation runs. Source after AI1_CI_ROLE is assigned. Every command and transfer runs there inside the installed CI sandbox as that role, through root-owned offload.py, and each function accepts only the options its callers use.
# AI1_* and ai1_* are retained compatibility names; this host is authoritative.
AI1_HOST=steve@192.168.68.73
AI1_OFFLOAD=/usr/local/lib/ai-ci/offload.py

# offload.py decides which roles exist. This only keeps a missing or malformed name, or one that is not a mutation role, off the remote command line.
ai1_role_check() {
  case "${AI1_CI_ROLE:-}" in
  '' | *[![:lower:][:digit:]-]*) ;;
  *-mutants) return 0 ;;
  esac
  printf 'CI transport: invalid or missing CI role\n' >&2
  return 2
}

# ai1_ssh_args [-o OPTION]... COMMAND... parses the ssh options and fills the caller's array ssh_args with the whole ssh argument list: the options, the host, and the role's command text. Each caller declares ssh_args local, so bash's dynamic scoping keeps it to that call. ai1_ssh and ai1_ssh_session share it so the two cannot drift.
ai1_ssh_args() {
  local command_text
  ssh_args=()
  while [ "$#" -gt 0 ]; do
    case "$1" in
    -o)
      if [ "$#" -lt 2 ]; then
        printf 'CI transport: -o requires a value\n' >&2
        return 2
      fi
      ssh_args+=("$1" "$2")
      shift 2
      ;;
    -*)
      printf 'CI transport: unsupported ssh option %s\n' "$1" >&2
      return 2
      ;;
    *) break ;;
    esac
  done
  if [ "$#" -eq 0 ]; then
    printf 'CI transport: a command is required\n' >&2
    return 2
  fi
  ai1_role_check || return $?
  if [ "$#" -eq 1 ]; then
    command_text="$1"
  else
    printf -v command_text '%q ' "$@"
  fi
  printf -v command_text 'sudo %s %q %q' "$AI1_OFFLOAD" "$AI1_CI_ROLE" "$command_text"
  ssh_args+=("$AI1_HOST" "$command_text")
}

# ai1_ssh [-o OPTION]... COMMAND...: run COMMAND on homelab-1 as the role. It never execs, because the reachability probe calls it in the foreground, where an exec would replace the calling script.
ai1_ssh() {
  local ssh_args
  ai1_ssh_args "$@" || return $?
  command ssh "${ssh_args[@]}"
}

# ai1_ssh_session [-o OPTION]... COMMAND...: the same command as ai1_ssh, ending in exec, for use as `ai1_ssh_session ... &`. It is refused in the top-level shell, where the exec would replace the calling script and skip its traps; a call inside a subshell still execs and replaces that subshell. bash runs a backgrounded function in a subshell; bash 3.2 skipped that fork for a trailing `command ssh` and bash 5 does not, so without the exec $! and jobs -p name the subshell and a kill leaves ssh running. It execs the binary type -P finds, because `exec command ssh` would run an external `command` where one exists and fail where none does. A missing ssh returns 127, as it does for ai1_ssh.
ai1_ssh_session() {
  local ssh_args ssh_bin
  if [ "${BASH_SUBSHELL:-0}" -eq 0 ]; then
    printf 'CI transport: ai1_ssh_session must run in the background\n' >&2
    return 2
  fi
  ai1_ssh_args "$@" || return $?
  ssh_bin="$(type -P ssh)" || {
    printf 'CI transport: ssh not found\n' >&2
    return 127
  }
  exec "$ssh_bin" "${ssh_args[@]}"
}

# ai1_push [OPTION]... LOCAL... REMOTE and ai1_pull [OPTION]... REMOTE LOCAL copy between local paths and a path relative to the role's home on homelab-1. The caller names which side is remote, so no operand is ever read as a host.
ai1_push() { ai1_rsync push "$@"; }
ai1_pull() { ai1_rsync pull "$@"; }

ai1_rsync() {
  local direction="$1" options=() operands=() operand rsync_path last
  shift
  for operand in "$@"; do
    case "$operand" in
    -a | -aR | --delete | --force | --delete-excluded | --no-times | --omit-dir-times | --timeout=* | --exclude=* | --filter=*) options+=("$operand") ;;
    -*)
      printf 'CI transport: unsupported rsync option %s\n' "$operand" >&2
      return 2
      ;;
    # rsync reads a colon before any slash as a host; ./ keeps a relative path local.
    /*) operands+=("$operand") ;;
    *) operands+=("./$operand") ;;
    esac
  done
  if [ "${#operands[@]}" -lt 2 ] || { [ "$direction" = pull ] && [ "${#operands[@]}" -ne 2 ]; }; then
    printf 'CI transport: %s needs local and remote paths\n' "ai1_$direction" >&2
    return 2
  fi
  ai1_role_check || return $?
  if [ "$direction" = push ]; then
    last=$((${#operands[@]} - 1))
    operands[last]="$AI1_HOST:${operands[last]#./}"
  else
    operands[0]="$AI1_HOST:${operands[0]#./}"
  fi
  printf -v rsync_path 'sudo %s %q rsync' "$AI1_OFFLOAD" "$AI1_CI_ROLE"
  command rsync --rsync-path="$rsync_path" ${options[@]+"${options[@]}"} "${operands[@]}"
}
