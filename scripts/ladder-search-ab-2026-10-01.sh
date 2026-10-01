#!/usr/bin/env bash
# F1 A/B: ladder --search off vs on, on the 19-dim SOTA bundle.
set -uo pipefail
cd "$(git rev-parse --show-toplevel)"

LOCKDIR="${TMPDIR:-/tmp}/chameleon-f1-ab.lock"
if ! mkdir "$LOCKDIR" 2>/dev/null; then
  echo "another F1 A/B is already running"; exit 0
fi
trap 'rm -rf "$LOCKDIR"' EXIT INT TERM

log() { echo "[$(date "+%H:%M:%S")] $*"; }

BUNDLE="$PWD/artifacts/agent-honest-19dim"
export CHAM_AGENT_BUNDLE="$BUNDLE"

log "ladder --fast --agent full (search OFF)"
target/release/chameleon ladder --fast --agent full \
  > artifacts/ladder-f1-search-off.log 2>&1 || log "  FAILED"

log "ladder --fast --agent full --search"
target/release/chameleon ladder --fast --agent full --search \
  > artifacts/ladder-f1-search-on.log 2>&1 || log "  FAILED"

log "DONE"
