#!/usr/bin/env bash
# F3 fix: retrain tiny robust at 5M with the opponent-node average
# accumulation, then LBR. Runs after the 50M curve finishes (contention).
set -uo pipefail
cd "$(git rev-parse --show-toplevel)"

LOCKDIR="${TMPDIR:-/tmp}/chameleon-f3-retrain.lock"
if ! mkdir "$LOCKDIR" 2>/dev/null; then
  echo "another F3 retrain is already running"; exit 0
fi
trap 'rm -rf "$LOCKDIR"' EXIT INT TERM

log() { echo "[$(date "+%H:%M:%S")] $*"; }

# Wait for the LBR curve to finish so we are not competing for cores
# with a 4-worker 50M train.
log "waiting for lbr-curve-rerun to finish..."
while pgrep -f "lbr-curve-rerun" >/dev/null; do sleep 120; done
log "lbr-curve-rerun done"

CFG=config/abstraction-tiny.toml
BUCKETS=artifacts/buckets-tiny
SEED=7
THREADS=4

out="artifacts/par-5M-f3"
if [ -f "$out/robust-7/policy/policy.bin" ]; then
  log "already trained, skipping"
else
  log "=== train 5M tiny robust (F3 accumulation site) ==="
  mkdir -p "$out"
  if ! target/release/chameleon train-bp \
      --mode robust --iters 5000000 --depth 100 --seed "$SEED" \
      --config "$CFG" --buckets "$BUCKETS" \
      --out "$out" \
      --threads "$THREADS" --thread-mode hogwild \
      > "artifacts/par-5M-f3.log" 2>&1
  then
    log "  training FAILED"; exit 1
  fi
  log "  training done"
fi

log "=== LBR (1000 deals) ==="
CHAM_EXPLOIT_BP="$PWD/$out/robust-7/policy" \
CHAM_EXPLOIT_BUCKETS="$PWD/$BUCKETS" \
CHAM_EXPLOIT_CONFIG="$PWD/$CFG" \
CHAM_EXPLOIT_DEALS=1000 \
  cargo bench -q -p cham-blueprint --bench exploitability \
  > "artifacts/par-5M-f3-lbr.criterion.log" \
  2> "artifacts/par-5M-f3-lbr.stderr.log"
grep "exploitability\[bp" "artifacts/par-5M-f3-lbr.stderr.log" \
  > "artifacts/par-5M-f3-lbr.log"
log "LBR:"
cat "artifacts/par-5M-f3-lbr.log"

log "DONE"
