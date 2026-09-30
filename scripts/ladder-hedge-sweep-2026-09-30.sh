#!/usr/bin/env bash
# Hedge threshold sweep (2026-09-30).
#
# The full-hedged ladder was catastrophic (-1800 mean, worse than
# robust-only). This script sweeps CHAM_HEDGE_THRESHOLD to determine
# whether the failure is a tuning problem or a logic bug:
#
#   * threshold = 0.0  -> always take argmax path -> should match `full` (+6587)
#   * threshold = 1.0  -> always take mixture path -> should match `full-mixture`
#   * anything in between -> interpolate
#
# If 0.0 does NOT match `full`, the hedged decision logic is different
# from the argmax decision logic — a bug.
# If 1.0 does NOT match `full-mixture`, the fallback mixture is
# different from the standalone mixture routing — a bug.
# If both endpoints match, the failure is just an intermediate
# threshold hitting a pathological mixture.
set -uo pipefail
cd "$(git rev-parse --show-toplevel)"

# Concurrency guard
LOCKDIR="${TMPDIR:-/tmp}/chameleon-hedge-sweep.lock"
if ! mkdir "$LOCKDIR" 2>/dev/null; then
  echo "another hedge-sweep is already running (lock=$LOCKDIR); exiting"
  exit 0
fi
trap 'rm -rf "$LOCKDIR"' EXIT INT TERM

log() { echo "[$(date "+%H:%M:%S")] $*"; }

BUNDLE="$PWD/artifacts/agent-honest"
export CHAM_AGENT_BUNDLE="$BUNDLE"

for thr in 0.00 0.20 0.50 0.80 1.00; do
  log "CHAM_HEDGE_THRESHOLD=$thr"
  CHAM_HEDGE_THRESHOLD="$thr" \
    target/release/chameleon ladder --fast --agent full-hedged \
    > "artifacts/ladder-hedge-thr${thr}.log" 2>&1 || \
    log "  ladder failed for thr=$thr"
done

log "DONE"
