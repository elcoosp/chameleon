#!/usr/bin/env bash
# A/B test: argmax (mode) vs sample-expert (sample σ) on the 19dim SOTA bundle.
set -uo pipefail
cd "$(git rev-parse --show-toplevel)"

LOCKDIR="${TMPDIR:-/tmp}/chameleon-f7-ab.lock"
if ! mkdir "$LOCKDIR" 2>/dev/null; then
  echo "another f7-ab is already running"; exit 0
fi
trap 'rm -rf "$LOCKDIR"' EXIT INT TERM

log() { echo "[$(date "+%H:%M:%S")] $*"; }

BUNDLE="$PWD/artifacts/agent-honest-19dim"
export CHAM_AGENT_BUNDLE="$BUNDLE"

for agent in argmax sample-expert; do
  log "ladder --fast --agent $agent"
  target/release/chameleon ladder --fast --agent "$agent" \
    > "artifacts/ladder-19dim-$agent.log" 2>&1 || \
    log "  ladder failed for $agent"
done

log "DONE"
