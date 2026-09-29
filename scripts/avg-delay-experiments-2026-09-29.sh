#!/usr/bin/env bash
# Averaging-delay experiments (2026-09-29).
#
# Tests whether the Linear CFR+ delay (D = T/4) is what discards the
# mixed pre-freeze phase at long runs. Runs two 20M tiny robust trains,
# one with delay removed and one with a uniform average, and benches
# each. Runs sequentially after the current jobs finish.
#
# See docs/plans/AVG-DELAY-VS-FREEZE-2026-09-29.md.
set -uo pipefail
cd "$(git rev-parse --show-toplevel)"

log() { echo "[$(date "+%H:%M:%S")] $*"; }

# Wait for the eps experiments to finish first.
log "waiting for eps02 experiments to finish..."
while pgrep -f "par-20M-eps02|par-5M-eps02" >/dev/null; do sleep 60; done
log "eps02 experiments done"

run_one() {
  local label="$1"
  local env_expr="$2"
  local out="artifacts/par-20M-${label}"

  if [ -f "${out}/robust-7/policy/policy.bin" ]; then
    log "${label}: already done, skipping train"
  else
    log "${label}: training 20M tiny robust"
    mkdir -p "${out}"
    env ${env_expr} target/release/chameleon train-bp \
      --mode robust --iters 20000000 --depth 100 --seed 7 \
      --config config/abstraction-tiny.toml \
      --buckets artifacts/buckets-tiny \
      --out "${out}" \
      --threads 4 --thread-mode hogwild \
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
}

# Test 1: no delay (weights ramp from iteration 0)
run_one "delay0" "CHAM_AVG_DELAY=0"

# Test 2: uniform average (every iteration weighted equally)
run_one "avguniform" "CHAM_AVG_UNIFORM=1"

log "PIPELINE DONE"
