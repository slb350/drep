#!/usr/bin/env bash
# Exercise fail-closed parsing without SSH, rsync, credentials, or filesystem I/O.
set -euo pipefail
AI1_CI_ROLE=drep-mutants
# shellcheck source=scripts/mutants-ai1-transport.sh
. scripts/mutants-ai1-transport.sh
command() { printf '<%s>' "$@"; }
expect_refusal() {
  local status=0 output
  output=$("$@" 2>/dev/null) || status=$?
  [[ $status == 2 && -z $output ]]
}
expect_refusal ssh -t steve@192.168.68.88 true
expect_refusal ssh -o
expect_refusal ssh
expect_refusal rsync --rsync-path=sh source steve@192.168.68.88:dest
output=$(ssh -o BatchMode=yes steve@192.168.68.88 'printf "%s" "literal $ text"')
[[ $output == *'sudo /usr/local/lib/ai-ci/offload.py drep-mutants'* ]]
[[ $output == *'literal'* ]]
output=$(ssh -o BatchMode=yes explicit-old-host true)
[[ $output == '<ssh><-o><BatchMode=yes><explicit-old-host><true>' ]]
output=$(rsync -a source steve@192.168.68.88:dest)
[[ $output == *'<--rsync-path=sudo /usr/local/lib/ai-ci/offload.py drep-mutants rsync>'* ]]
output=$(rsync -a explicit-old-host:source dest)
[[ $output == '<rsync><-a><explicit-old-host:source><dest>' ]]
output=$(ssh 192.168.68.88 true)
[[ $output == *'sudo /usr/local/lib/ai-ci/offload.py drep-mutants'* ]]
output=$(rsync homelab-ai-1.local:source dest)
[[ $output == *'<--rsync-path=sudo /usr/local/lib/ai-ci/offload.py drep-mutants rsync>'* ]]
expect_refusal rsync rsync://192.168.68.88/module dest
expect_refusal rsync 192.168.68.88::module dest
expect_refusal rsync homelab-ai-1.local::module dest
expect_refusal rsync steve@192.168.68.88::module dest
AI1_CI_ROLE='bad; role'
expect_refusal ssh steve@192.168.68.88 true
expect_refusal rsync source steve@192.168.68.88:dest
