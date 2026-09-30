#!/usr/bin/env bash
# Router sharpening-temperature sweep on agent-honest-19dim (2026-09-30).
# Runs AFTER the 5M expert retrain to avoid CPU contention.
set -uo pipefail
cd "$(git rev-parse --show-toplevel)"

LOCKDIR="${TMPDIR:-/tmp}/chameleon-19dim-temp.lock"
if ! mkdir "$LOCKDIR" 2>/dev/null; then
  echo "another 19dim-temp sweep is already running"; exit 0
fi
trap 'rm -rf "$LOCKDIR"' EXIT INT TERM

log() { echo "[$(date "+%H:%M:%S")] $*"; }

log "waiting for retrain-tiny-5M-experts to finish..."
while pgrep -f "retrain-tiny-5M-experts" >/dev/null; do sleep 120; done
log "retrain done"

BUNDLE="$PWD/artifacts/agent-honest-19dim"
export CHAM_AGENT_BUNDLE="$BUNDLE"

for t in 0.5 0.7 1.0; do
  log "CHAM_ROUTER_TEMP=$t  agent=full"
  CHAM_ROUTER_TEMP="$t" \
    target/release/chameleon ladder --fast --agent full \
    > "artifacts/ladder-19dim-full-temp${t}.log" 2>&1 || \
    log "  failed for t=$t"
done

log "DONE"
