#!/usr/bin/env bash
# Re-run the tiny robust LBR curve at 500k / 5M / 20M / 50M with the
# F3/F4/F5 trainer fixes landed. See COMPETITIVE-REVIEW-2026-10-01.md.
#
# Each run: train-bp at tiny robust, then 1000-deal LBR at depth 100.
# Sequential; uses hogwild 4 workers. Total wall ~10-12h on this box.
set -uo pipefail
cd "$(git rev-parse --show-toplevel)"

LOCKDIR="${TMPDIR:-/tmp}/chameleon-lbr-curve.lock"
if ! mkdir "$LOCKDIR" 2>/dev/null; then
  echo "another LBR curve is already running"; exit 0
fi
trap 'rm -rf "$LOCKDIR"' EXIT INT TERM

log() { echo "[$(date "+%H:%M:%S")] $*"; }

CFG=config/abstraction-tiny.toml
BUCKETS=artifacts/buckets-tiny
SEED=7
THREADS=4

for iters in 500000 5000000 20000000 50000000; do
  out="artifacts/par-curve2-${iters}"

  if [ ! -f "$out/robust-7/policy/policy.bin" ]; then
    log "=== train $iters iters ==="
    mkdir -p "$out"
    target/release/chameleon train-bp \
      --mode robust --iters "$iters" --depth 100 --seed "$SEED" \
      --config "$CFG" --buckets "$BUCKETS" \
      --out "$out" \
      --threads "$THREADS" --thread-mode hogwild \
      > "artifacts/par-curve2-${iters}.log" 2>&1
    if [ $? -ne 0 ]; then
      log "  train failed for $iters; skipping LBR"
      continue
    fi
    log "  $iters train done"
  else
    log "$iters already trained"
  fi

  log "=== LBR $iters (1000 deals) ==="
  CHAM_EXPLOIT_BP="$PWD/${out}/robust-7/policy" \
  CHAM_EXPLOIT_BUCKETS="$PWD/$BUCKETS" \
  CHAM_EXPLOIT_CONFIG="$PWD/$CFG" \
  CHAM_EXPLOIT_DEALS=1000 \
    cargo bench -q -p cham-blueprint --bench exploitability \
    > "artifacts/par-curve2-${iters}-lbr.criterion.log" \
    2> "artifacts/par-curve2-${iters}-lbr.stderr.log"
  grep "exploitability\[bp" "artifacts/par-curve2-${iters}-lbr.stderr.log" \
    > "artifacts/par-curve2-${iters}-lbr.log"
  log "  $iters LBR done:"
  cat "artifacts/par-curve2-${iters}-lbr.log"
done

log "=== COMPARISON ==="
{
  echo "=== tiny robust LBR curve, F3/F4/F5 fixed (2026-10-01) ==="
  echo
  echo "iters | seat0 | seat1 | mean | note"
  echo "------|-------|-------|------|-----"
  for iters in 500000 5000000 20000000 50000000; do
    log_file="artifacts/par-curve2-${iters}-lbr.log"
    if [ -f "$log_file" ]; then
      s0=$(grep "depth=100 seat=0" "$log_file" | awk '{print $NF}')
      s1=$(grep "depth=100 seat=1" "$log_file" | awk '{print $NF}')
      echo "$iters | $s0 | $s1 | (see above)"
    fi
  done
} > docs/plans/LBR-CURVE-FIXED-2026-10-01.md

log "DONE — see docs/plans/LBR-CURVE-FIXED-2026-10-01.md"
