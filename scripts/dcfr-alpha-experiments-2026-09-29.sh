#!/usr/bin/env bash
# DCFR regret-discount sweep (2026-09-29).
#
# Third lever against the RM+ freeze. DCFR (Discounted CFR, Brown &
# Sandholm 2019) discounts old positive regrets by alpha before adding
# new deltas. That directly counters the freeze mechanism: even when one
# action dominates, its accumulated regret gets discounted over time,
# so it never becomes permanent.
#
# The workspace already has --regret-discount (default 1.0 = pure CFR+).
# This script runs the same 20M tiny robust train at alpha = 0.9 and
# 0.5, then benches both.
#
# Compare against the no-eps 20M (13319 / 14706 at 1000 deals).
set -uo pipefail
cd "$(git rev-parse --show-toplevel)"

log() { echo "[$(date "+%H:%M:%S")] $*"; }

log "waiting for freeze-evolution pipeline..."
while pgrep -f "freeze-evolution-2026-09-29" >/dev/null; do sleep 60; done
log "freeze-evolution done"

run_one() {
  local alpha="$1"
  local label="alpha${alpha/./}"
  local out="artifacts/par-20M-${label}"

  if [ -f "${out}/robust-7/policy/policy.bin" ]; then
    log "${label}: already done, skipping train"
  else
    log "${label}: training 20M tiny robust with --regret-discount ${alpha}"
    mkdir -p "${out}"
    target/release/chameleon train-bp \
      --mode robust --iters 20000000 --depth 100 --seed 7 \
      --config config/abstraction-tiny.toml \
      --buckets artifacts/buckets-tiny \
      --out "${out}" \
      --threads 4 --thread-mode hogwild \
      --regret-discount "${alpha}" \
      > "artifacts/par-20M-${label}.log" 2>&1
  fi

  log "${label}: LBR (1000 deals)"
  CHAM_EXPLOIT_BP="$PWD/${out}/robust-7/policy" \
  CHAM_EXPLOIT_BUCKETS="$PWD/artifacts/buckets-tiny" \
  CHAM_EXPLOIT_CONFIG="$PWD/config/abstraction-tiny.toml" \
  CHAM_EXPLOIT_DEALS=1000 \
    cargo bench -q -p cham-blueprint --bench exploitability \
    > "artifacts/par-20M-${label}-lbr.criterion.log" \
    2> "artifacts/par-20M-${label}-lbr.stderr.log"
  grep "exploitability\[bp" "artifacts/par-20M-${label}-lbr.stderr.log" \
    > "artifacts/par-20M-${label}-lbr.log"

  log "${label}: rm_freeze"
  /tmp/rm_freeze/target/release/rm_freeze "${out}/robust-7/table.snap" \
    > "artifacts/par-20M-${label}-freeze.txt" 2>&1
}

run_one "0.9"
run_one "0.5"

log "PIPELINE DONE"
