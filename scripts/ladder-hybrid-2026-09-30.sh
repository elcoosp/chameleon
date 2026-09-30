#!/usr/bin/env bash
# Ladder measurement: 500k experts + 5M robust fallback (2026-09-30).
#
# Question: does replacing the 500k robust fallback with the 5M robust
# fallback change the shipped ladder? The mixture/argmax primarily uses
# the experts, so the fallback's contribution might be small.
set -uo pipefail
cd "$(git rev-parse --show-toplevel)"

log() { echo "[$(date "+%H:%M:%S")] $*"; }

# Concurrency guard (2026-09-30): refuse a second concurrent invocation
# so two ladders don't race on the same output logs.
LOCKDIR="${TMPDIR:-/tmp}/chameleon-ladder-hybrid.lock"
if ! mkdir "$LOCKDIR" 2>/dev/null; then
  log "another ladder-hybrid is already running (lock=$LOCKDIR); exiting"
  exit 0
fi
trap 'rm -rf "$LOCKDIR"' EXIT INT TERM

for agent in full full-mixture robust-only; do
  log "ladder --fast --agent $agent @ agent-honest-5Mrobust"
  CHAM_AGENT_BUNDLE="$PWD/artifacts/agent-honest-5Mrobust" \
    target/release/chameleon ladder --fast --agent "$agent" \
    > "artifacts/ladder-hybrid-5Mrobust-$agent.log" 2>&1 \
    || echo "  failed"
done

log "DONE"
