#!/bin/sh

set -eu

case "${AGENT_LOWMEM_FIXTURE_MODE:-}" in
  exit)
    printf '%s\n' "$$" >"$AGENT_LOWMEM_FIXTURE_EVIDENCE"
    exit 17
    ;;
  self-signal)
    printf '%s\n' "$$" >"$AGENT_LOWMEM_FIXTURE_EVIDENCE"
    kill -TERM "$$"
    ;;
  leave-descendant)
    /bin/sleep 60 &
    descendant=$!
    printf '%s %s\n' "$$" "$descendant" >"$AGENT_LOWMEM_FIXTURE_EVIDENCE"
    exit 0
    ;;
  ignore-term)
    trap '' TERM
    (
      /bin/sleep 0.25
      kill -INT "$PPID"
    ) &
    notifier=$!
    printf '%s %s\n' "$$" "$notifier" >"$AGENT_LOWMEM_FIXTURE_EVIDENCE"
    while :; do
      /bin/sleep 1
    done
    ;;
  *)
    exit 64
    ;;
esac
