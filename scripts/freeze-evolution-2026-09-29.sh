#!/usr/bin/env bash
# Freeze-evolution diagnostic (2026-09-29).
#
# Trains 20M tiny robust with checkpoints every 2M iterations, then runs
# rm_freeze on each snapshot to see how the RM+ freeze evolves. This
# tells us WHERE the freeze starts, which is the input the averaging
# schedule fix needs (if the freeze starts around T/4, the delay is
# discarding the useful pre-freeze phase).
#
# Also runs a 5M warmfix comparison: does the insert-only warmup change
# affect the current peak at 5M, or is it only visible at long runs?
set -uo pipefail
cd "$(git rev-parse --show-toplevel)"

log() { echo "[$(date "+%H:%M:%S")] $*"; }

# Wait for the delay experiments to finish.
log "waiting for avg-delay experiments..."
while pgrep -f "avg-delay-experiments" >/dev/null; do sleep 60; done
log "avg-delay done"

# ---- Experiment 1: freeze evolution ----
CKPT_DIR="artifacts/freeze-diag"
if [ ! -f "${CKPT_DIR}/iter-20000000/table.snap" ]; then
  log "freeze-diag: training 20M with checkpoints every 2M"
  rm -rf "${CKPT_DIR}"
  mkdir -p "${CKPT_DIR}"
  CHAM_TRAIN_EPS=0 target/release/chameleon train-bp \
    --mode robust --iters 20000000 --depth 100 --seed 7 \
    --config config/abstraction-tiny.toml \
    --buckets artifacts/buckets-tiny \
    --out "${CKPT_DIR}" \
    --threads 4 --thread-mode hogwild \
    --checkpoint-every 2000000 --checkpoint-dir "${CKPT_DIR}/checkpoints" \
    > "artifacts/freeze-diag.log" 2>&1
fi

log "freeze-diag: analyzing checkpoints"
{
  echo "# Freeze evolution over a 20M run (checkpoints every 2M iters)"
  echo
  for i in 2000000 4000000 6000000 8000000 10000000 12000000 14000000 16000000 18000000 20000000; do
    snap="${CKPT_DIR}/checkpoints/iter-${i}/table.snap"
    [ -f "$snap" ] || continue
    echo "## iter ${i}"
    /tmp/rm_freeze/target/release/rm_freeze "$snap" 2>&1
    echo
  done
} > docs/plans/FREEZE-EVOLUTION-2026-09-29.raw.txt
log "freeze-diag: raw output -> docs/plans/FREEZE-EVOLUTION-2026-09-29.raw.txt"

# ---- Experiment 2: 5M warmfix ----
if [ ! -f "artifacts/par-5M-warmfix/robust-7/policy/policy.bin" ]; then
  log "5M warmfix: training"
  mkdir -p artifacts/par-5M-warmfix
  target/release/chameleon train-bp \
    --mode robust --iters 5000000 --depth 100 --seed 7 \
    --config config/abstraction-tiny.toml \
    --buckets artifacts/buckets-tiny \
    --out artifacts/par-5M-warmfix \
    --threads 4 --thread-mode hogwild \
    > artifacts/par-5M-warmfix.log 2>&1
fi
log "5M warmfix: LBR"
CHAM_EXPLOIT_BP="$PWD/artifacts/par-5M-warmfix/robust-7/policy" \
CHAM_EXPLOIT_BUCKETS="$PWD/artifacts/buckets-tiny" \
CHAM_EXPLOIT_CONFIG="$PWD/config/abstraction-tiny.toml" \
CHAM_EXPLOIT_DEALS=1000 \
  cargo bench -q -p cham-blueprint --bench exploitability \
  > "artifacts/par-5M-warmfix-lbr.criterion.log" \
  2> "artifacts/par-5M-warmfix-lbr.stderr.log"
grep "exploitability\[bp" "artifacts/par-5M-warmfix-lbr.stderr.log" \
  > "artifacts/par-5M-warmfix-lbr.log"

log "PIPELINE DONE"
