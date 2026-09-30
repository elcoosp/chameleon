#!/usr/bin/env bash
# Ladder test for the 19-dim honest router (2026-09-30).
set -uo pipefail
cd "$(git rev-parse --show-toplevel)"

LOCKDIR="${TMPDIR:-/tmp}/chameleon-19dim-ladder.lock"
if ! mkdir "$LOCKDIR" 2>/dev/null; then
  echo "another 19dim ladder is already running"; exit 0
fi
trap 'rm -rf "$LOCKDIR"' EXIT INT TERM

log() { echo "[$(date "+%H:%M:%S")] $*"; }

BUNDLE="$PWD/artifacts/agent-honest-19dim"
export CHAM_AGENT_BUNDLE="$BUNDLE"

for agent in full full-mixture robust-only; do
  log "ladder --fast --agent $agent @ agent-honest-19dim"
  target/release/chameleon ladder --fast --agent "$agent" \
    > "artifacts/ladder-19dim-$agent.log" 2>&1 || \
    log "  failed for $agent"
done

log "DONE"
