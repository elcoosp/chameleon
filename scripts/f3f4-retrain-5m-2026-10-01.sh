#!/usr/bin/env bash
# F3+F4+F6a: retrain tiny robust at 5M with:
#   * F3 (avg accumulation at opponent node)
#   * F4 (f64 strategy arena, no renorm)
#   * F6a (raise cap counts re-raises only)
# Then LBR. Waits for the old-binary LBR curve first.
set -uo pipefail
cd "$(git rev-parse --show-toplevel)"

LOCKDIR="${TMPDIR:-/tmp}/chameleon-f3f4-retrain.lock"
if ! mkdir "$LOCKDIR" 2>/dev/null; then
  echo "another F3F4 retrain is already running"; exit 0
fi
trap 'rm -rf "$LOCKDIR"' EXIT INT TERM

log() { echo "[$(date "+%H:%M:%S")] $*"; }

log "waiting for lbr-curve-rerun to finish..."
while pgrep -f "lbr-curve-rerun" >/dev/null; do sleep 120; done
log "lbr-curve-rerun done"

CFG=config/abstraction-tiny.toml
BUCKETS=artifacts/buckets-tiny
SEED=7
THREADS=4

for iters in 500000 5000000 20000000; do
  out="artifacts/par-f3f4-${iters}"
  if [ ! -f "$out/robust-7/policy/policy.bin" ]; then
    log "=== train $iters (F3+F4+F6a) ==="
    mkdir -p "$out"
    if ! target/release/chameleon train-bp \
        --mode robust --iters "$iters" --depth 100 --seed "$SEED" \
        --config "$CFG" --buckets "$BUCKETS" \
        --out "$out" \
        --threads "$THREADS" --thread-mode hogwild \
        > "artifacts/par-f3f4-${iters}.log" 2>&1
    then
      log "  train $iters FAILED"; continue
    fi
    log "  $iters trained"
  fi

  log "=== LBR $iters ==="
  CHAM_EXPLOIT_BP="$PWD/${out}/robust-7/policy" \
  CHAM_EXPLOIT_BUCKETS="$PWD/$BUCKETS" \
  CHAM_EXPLOIT_CONFIG="$PWD/$CFG" \
  CHAM_EXPLOIT_DEALS=1000 \
    cargo bench -q -p cham-blueprint --bench exploitability \
    > "artifacts/par-f3f4-${iters}-lbr.criterion.log" \
    2> "artifacts/par-f3f4-${iters}-lbr.stderr.log"
  grep "exploitability\[bp" "artifacts/par-f3f4-${iters}-lbr.stderr.log" \
    > "artifacts/par-f3f4-${iters}-lbr.log"
  log "  LBR:"; cat "artifacts/par-f3f4-${iters}-lbr.log"
done

log "DONE"
