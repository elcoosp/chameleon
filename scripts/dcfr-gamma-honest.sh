#!/usr/bin/env bash
# Post-RBP-fix honest re-measure of the gamma default. Two cells:
#   gamma=0.9 (default) vs gamma=1.0 (uniform linear averaging over last 75%)
# at the 3M-iter scale on the tiny abstraction, so the comparison is
# directly against the recorded honest-lbr-tiny-3M-s7 entry.
#
# Deliberately TWO cells, not a grid: the analysis in
# docs/reports/20260927-averaging-gamma-analysis.md shows that intermediate
# gammas (0.99, 0.999) are also snapshots, just wider ones. γ=1.0 is the
# only setting that actually averages.
set -uo pipefail
cd "$(git rev-parse --show-toplevel)"
LOG=artifacts/dcfr-gamma-honest.log
: > "$LOG"
log(){ echo "[$(date +%H:%M:%S)] $*" | tee -a "$LOG"; }

CFG=config/abstraction-tiny.toml
BUCKETS=artifacts/buckets-tiny
OUTBASE=artifacts/dcfr-gamma-honest
ITERS=3000000
SEED=7
THREADS=4

for gamma in 0.9 1.0; do
  RUNDIR="$OUTBASE/g${gamma}"
  if [ -f "$RUNDIR/robust-7/policy/policy.bin" ]; then
    log "reuse $RUNDIR"
  else
    log "train robust 3M iters gamma=$gamma"
    nice -n 15 cargo run -q --release -p cham-cli -- train-bp \
      --mode robust --iters "$ITERS" --depth 100 --seed "$SEED" \
      --config "$CFG" --buckets "$BUCKETS" --avg-gamma "$gamma" \
      --out "$RUNDIR" --threads "$THREADS" 2>&1 | tee -a "$LOG" | tail -1
  fi
  log "bench gamma=$gamma"
  CHAM_EXPLOIT_BP="$PWD/$RUNDIR/robust-7/policy" \
  CHAM_EXPLOIT_BUCKETS="$PWD/$BUCKETS" \
  CHAM_EXPLOIT_CONFIG="$PWD/$CFG" \
  CHAM_EXPLOIT_DEALS=200 \
    cargo bench -q -p cham-blueprint --bench exploitability 2>&1 \
    | grep -E 'exploitability\[bp depth=100' | tee -a "$LOG"
done
log "DONE"
