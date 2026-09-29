#!/usr/bin/env bash
# Standalone freeze-evolution diagnostic (2026-09-29, retry).
#
# The original freeze-evolution script tried to run this at 23:08 but the
# release binary predated the --checkpoint-dir flag (only cargo check had
# been run, not cargo build --release). This version queues after the
# dcfr-alpha pipeline finishes, then trains 20M tiny robust with
# checkpoints every 2M iters and runs /tmp/rm_freeze on each.
set -uo pipefail
cd "$(git rev-parse --show-toplevel)"

log() { echo "[$(date "+%H:%M:%S")] $*"; }

log "waiting for dcfr-alpha-experiments to finish..."
while pgrep -f "dcfr-alpha-experiments" >/dev/null; do sleep 60; done
log "dcfr-alpha done"

CKPT_DIR="artifacts/freeze-diag"
if [ ! -f "${CKPT_DIR}/checkpoints/iter-20000000/table.snap" ]; then
  log "freeze-diag: training 20M with checkpoints every 2M"
  rm -rf "${CKPT_DIR}"
  mkdir -p "${CKPT_DIR}"
  CHAM_TRAIN_EPS=0 target/release/chameleon train-bp \
    --mode robust --iters 20000000 --depth 100 --seed 7 \
    --config config/abstraction-tiny.toml \
    --buckets artifacts/buckets-tiny \
    --out "${CKPT_DIR}" \
    --threads 4 --thread-mode hogwild \
    --checkpoint-every 2000000 \
    --checkpoint-dir "${CKPT_DIR}/checkpoints" \
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
log "DONE"
