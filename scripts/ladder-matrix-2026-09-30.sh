#!/usr/bin/env bash
# Ladder matrix for agent-full-honest (2026-09-30).
#
# Runs `ladder --fast` on the SAME bundle with every routing variant
# so we can separate the routing contribution from the bundle
# contribution:
#
#   full           -> argmax over 4 experts (SOTA routing)
#   full-mixture   -> sharpened-softmax mixture
#   full-hedged    -> argmax if top weight > threshold, else mixture
#   robust-only    -> ignore experts + router, use robust policy
#
# Then we know how much of the 09-28 +7146 came from routing vs from
# the specific experts in this bundle.
set -uo pipefail
cd "$(git rev-parse --show-toplevel)"

log() { echo "[$(date "+%H:%M:%S")] $*"; }

BUNDLE="${CHAM_AGENT_BUNDLE:-$PWD/artifacts/agent-full-honest}"
export CHAM_AGENT_BUNDLE="$BUNDLE"

log "bundle = $BUNDLE"

for agent in full full-mixture full-hedged robust-only; do
  log "ladder --fast --agent $agent"
  target/release/chameleon ladder --fast --agent "$agent" \
    > "artifacts/ladder-agent-full-honest-$agent.log" 2>&1 \
    || echo "  ladder for agent=$agent failed"
done

log "DONE"
