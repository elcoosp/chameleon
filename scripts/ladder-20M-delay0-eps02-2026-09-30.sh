#!/usr/bin/env bash
# Ladder for the 20M delay0+eps02 robust policy (2026-09-30).
#
# The handoff asks: does the LBR-improving 20M robust policy actually
# matter on the shipped archetype ladder? This measures robust-only
# on the 20M delay0+eps02 bundle and compares to agent-honest's +720.
set -uo pipefail
cd "$(git rev-parse --show-toplevel)"

# Concurrency lock (see HANDOFF 6.3).
LOCKDIR="${TMPDIR:-/tmp}/chameleon-ladder-20m.lock"
if ! mkdir "$LOCKDIR" 2>/dev/null; then
  echo "another 20M ladder is already running (lock=$LOCKDIR); exiting"
  exit 0
fi
trap 'rm -rf "$LOCKDIR"' EXIT INT TERM

log() { echo "[$(date "+%H:%M:%S")] $*"; }

BUNDLE="$PWD/artifacts/agent-robust-20M-delay0-eps02"
export CHAM_AGENT_BUNDLE="$BUNDLE"
log "bundle = $BUNDLE"

for agent in robust-only full; do
  log "ladder --fast --agent $agent"
  target/release/chameleon ladder --fast --agent "$agent" \
    > "artifacts/ladder-20M-delay0-eps02-$agent.log" 2>&1 \
    || log "  ladder for $agent failed"
done

log "DONE"
