#!/usr/bin/env bash
# Unattended pipeline. Waits for the current jobs to finish, then runs:
#   1. Medium abstraction at 20M iters (parallel) — new SOTA candidate
#   2. Tiny abstraction at 50M iters (parallel) — fills the tiny curve
#   3. LBR on both
# Idempotent: skips anything already on disk.
set -uo pipefail
cd "$(git rev-parse --show-toplevel)"

log() { echo "[$(date '+%H:%M:%S')] $*"; }

# ---- wait for prior jobs ----
log "waiting for par-20M and train-buckets-medium..."
while pgrep -f "artifacts/par-20M" >/dev/null 2>&1; do sleep 60; done
log "par-20M done"
while pgrep -f "train-buckets.*medium" >/dev/null 2>&1; do sleep 30; done
log "train-buckets-medium done"

# ---- LBR the 20M result ----
if [ -f artifacts/par-20M/robust-7/policy/policy.bin ]; then
  log "LBR par-20M"
  CHAM_EXPLOIT_BP="$PWD/artifacts/par-20M/robust-7/policy" \
  CHAM_EXPLOIT_BUCKETS="$PWD/artifacts/buckets-tiny" \
  CHAM_EXPLOIT_CONFIG="$PWD/config/abstraction-tiny.toml" \
  CHAM_EXPLOIT_DEALS=200 \
    cargo bench -q -p cham-blueprint --bench exploitability 2>&1 \
    | grep -E "exploitability\[bp" > artifacts/par-20M-lbr.log
fi

# ---- medium 20M ----
if [ ! -f artifacts/par-medium-20M/robust-7/policy/policy.bin ]; then
  mkdir -p artifacts/par-medium-20M
  log "medium 20M training"
  target/release/chameleon train-bp \
    --mode robust --iters 20000000 --depth 100 --seed 7 \
    --config config/abstraction-medium.toml \
    --buckets artifacts/buckets-medium \
    --out artifacts/par-medium-20M \
    --threads 4 --thread-mode hogwild \
    > artifacts/par-medium-20M.log 2>&1
  log "medium 20M LBR"
  CHAM_EXPLOIT_BP="$PWD/artifacts/par-medium-20M/robust-7/policy" \
  CHAM_EXPLOIT_BUCKETS="$PWD/artifacts/buckets-medium" \
  CHAM_EXPLOIT_CONFIG="$PWD/config/abstraction-medium.toml" \
  CHAM_EXPLOIT_DEALS=200 \
    cargo bench -q -p cham-blueprint --bench exploitability 2>&1 \
    | grep -E "exploitability\[bp" > artifacts/par-medium-20M-lbr.log
fi

# ---- tiny 50M ----
if [ ! -f artifacts/par-50M/robust-7/policy/policy.bin ]; then
  mkdir -p artifacts/par-50M
  log "tiny 50M training"
  target/release/chameleon train-bp \
    --mode robust --iters 50000000 --depth 100 --seed 7 \
    --config config/abstraction-tiny.toml \
    --buckets artifacts/buckets-tiny \
    --out artifacts/par-50M \
    --threads 4 --thread-mode hogwild \
    > artifacts/par-50M.log 2>&1
  log "tiny 50M LBR"
  CHAM_EXPLOIT_BP="$PWD/artifacts/par-50M/robust-7/policy" \
  CHAM_EXPLOIT_BUCKETS="$PWD/artifacts/buckets-tiny" \
  CHAM_EXPLOIT_CONFIG="$PWD/config/abstraction-tiny.toml" \
  CHAM_EXPLOIT_DEALS=200 \
    cargo bench -q -p cham-blueprint --bench exploitability 2>&1 \
    | grep -E "exploitability\[bp" > artifacts/par-50M-lbr.log
fi

log "PIPELINE DONE"
