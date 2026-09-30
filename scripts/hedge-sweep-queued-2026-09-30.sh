#!/usr/bin/env bash
# Queued hedge-threshold sweep (2026-09-30, retry after the guard fix).
#
# The 12:12 sweep (scripts/ladder-hedge-sweep-2026-09-30.sh) silently
# measured CallBot because `full-hedged` wasn't in TRAINED_AGENTS. The
# guard is now fixed (dd167df). This script waits for the 5M expert
# retrain to finish, then re-runs the sweep with the corrected binary.
set -uo pipefail
cd "$(git rev-parse --show-toplevel)"

LOCKDIR="${TMPDIR:-/tmp}/chameleon-hedge-sweep-queued.lock"
if ! mkdir "$LOCKDIR" 2>/dev/null; then
  echo "another queued hedge sweep is already running (lock=$LOCKDIR); exiting"
  exit 0
fi
trap 'rm -rf "$LOCKDIR"' EXIT INT TERM

log() { echo "[$(date "+%H:%M:%S")] $*"; }

log "waiting for retrain-tiny-5M-experts to finish..."
while pgrep -f "retrain-tiny-5M-experts" >/dev/null; do sleep 60; done
log "retrain done"

# Fresh release binary check (the guard fix must be compiled in).
if [ ! -x target/release/chameleon ]; then
  log "ERROR: target/release/chameleon missing"; exit 1
fi
if ! target/release/chameleon collect --help 2>&1 | grep -q "raw-opponent-11"; then
  log "WARNING: release binary may be stale"
fi

BUNDLE="$PWD/artifacts/agent-honest"
export CHAM_AGENT_BUNDLE="$BUNDLE"

for thr in 0.00 0.20 0.50 0.80 1.00; do
  log "CHAM_HEDGE_THRESHOLD=$thr"
  CHAM_HEDGE_THRESHOLD="$thr" \
    target/release/chameleon ladder --fast --agent full-hedged \
    > "artifacts/ladder-hedge-FIXED-thr${thr}.log" 2>&1 || \
    log "  ladder failed for thr=$thr"
done

log "DONE"
