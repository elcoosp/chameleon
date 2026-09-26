#!/usr/bin/env bash
# EXP-011: train robust at each (α, γ) cell on the tiny abstraction, then
# evaluate the trained policy's LBR with the extended exploitability bench.
#
# Metric per cell: LBR (mb/hand) at depth 100, BR seat 0. Lower = harder to
# exploit. Baseline cell is (α=1.0, γ=0.9). Kill criterion: no cell beats
# baseline beyond noise → keep defaults.
set -uo pipefail
cd "$(git rev-parse --show-toplevel)"
LOG=artifacts/exp-011-sweep.log
: > "$LOG"
log() { echo "[$(date +%H:%M:%S)] $*" | tee -a "$LOG"; }

CFG=config/abstraction-tiny.toml
BUCKETS=artifacts/buckets-tiny
ITERS=100000
SEED=7
OUTBASE=artifacts/blueprints-exp-011
mkdir -p "$OUTBASE"

# α ∈ {1.0, 0.9, 0.5} × γ ∈ {1.0, 0.9, 0.5}
for alpha in 1.0 0.9 0.5; do
  for gamma in 1.0 0.9 0.5; do
    tag="a${alpha}_g${gamma}"
    rundir="$OUTBASE/robust-a${alpha}-g${gamma}"
    mkdir -p "$rundir"
    log "=== cell α=$alpha γ=$gamma ==="
    cargo run -q --release -p cham-cli -- train-bp \
      --mode robust --iters "$ITERS" --depth 100 --seed "$SEED" \
      --config "$CFG" --buckets "$BUCKETS" \
      --regret-discount "$alpha" --avg-gamma "$gamma" \
      --out "$rundir" --threads 4 2>&1 | tee -a "$LOG" | tail -1
    pol="$rundir/robust-$SEED/policy"
    if [ ! -f "$pol/policy.bin" ]; then
      log "  MISSING artifact for cell $tag; skipping bench"
      continue
    fi
    log "  bench: $pol"
    CHAM_EXPLOIT_BP="$pol" \
    CHAM_EXPLOIT_BUCKETS="$BUCKETS" \
    CHAM_EXPLOIT_CONFIG="$CFG" \
      cargo bench -q -p cham-blueprint --bench exploitability 2>&1 \
      | grep -E 'exploitability\[bp ' | tee -a "$LOG"
  done
done

log "=== EXP-011 sweep DONE $(date) ==="
