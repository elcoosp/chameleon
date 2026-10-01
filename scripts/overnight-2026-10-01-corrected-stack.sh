#!/usr/bin/env bash
# Overnight: measure the F3+F4+F6a+F5-corrected stack with the F1-corrected
# metric. Every prior curve was measured against clairvoyant LBR on a
# different trainer + tree; this is the first honest baseline.
set -uo pipefail
cd "$(git rev-parse --show-toplevel)"

LOCKDIR="${TMPDIR:-/tmp}/chameleon-overnight-2026-10-01.lock"
if ! mkdir "$LOCKDIR" 2>/dev/null; then
  echo "another overnight queue is already running"; exit 0
fi
trap 'rm -rf "$LOCKDIR"' EXIT INT TERM

log() { echo "[$(date "+%H:%M:%S")] $*"; }

CFG=config/abstraction-tiny.toml
BUCKETS=artifacts/buckets-tiny
SEED=7
THREADS=4

train_tiny() {
  local iters="$1"
  local out="artifacts/par-f5-tiny-${iters}"
  if [ -f "$out/robust-7/policy/policy.bin" ]; then
    log "tiny $iters already trained"
  else
    log "=== tiny robust $iters (F3+F4+F6a+F5) ==="
    mkdir -p "$out"
    if ! target/release/chameleon train-bp \
        --mode robust --iters "$iters" --depth 100 --seed "$SEED" \
        --config "$CFG" --buckets "$BUCKETS" \
        --out "$out" --threads "$THREADS" --thread-mode hogwild \
        > "artifacts/par-f5-tiny-${iters}.log" 2>&1
    then
      log "  train FAILED for $iters"; return 1
    fi
    log "  done"
  fi
  return 0
}

train_tiny_dcfr() {
  local out="artifacts/par-f5-tiny-dcfr15"
  if [ -f "$out/robust-7/policy/policy.bin" ]; then
    log "tiny DCFR already trained"
  else
    log "=== tiny robust 20M with DCFR(1.5, 0.0) ==="
    mkdir -p "$out"
    if ! target/release/chameleon train-bp \
        --mode robust --iters 20000000 --depth 100 --seed "$SEED" \
        --config "$CFG" --buckets "$BUCKETS" \
        --out "$out" --threads "$THREADS" --thread-mode hogwild \
        --dcfr-alpha 1.5 --dcfr-beta 0.0 \
        > "artifacts/par-f5-tiny-dcfr15.log" 2>&1
    then
      log "  DCFR train FAILED"; return 1
    fi
    log "  done"
  fi
  return 0
}

train_medium() {
  local out="artifacts/par-f5-medium-20M"
  if [ -f "$out/robust-7/policy/policy.bin" ]; then
    log "medium 20M already trained"
  else
    log "=== medium robust 20M (F3+F4+F6a+F5) ==="
    mkdir -p "$out"
    if ! target/release/chameleon train-bp \
        --mode robust --iters 20000000 --depth 100 --seed "$SEED" \
        --config config/abstraction-medium.toml \
        --buckets artifacts/buckets-medium \
        --out "$out" --threads "$THREADS" --thread-mode hogwild \
        > "artifacts/par-f5-medium-20M.log" 2>&1
    then
      log "  medium train FAILED"; return 1
    fi
    log "  done"
  fi
  return 0
}

metric() {
  local out="$1" cfg="$2" buckets="$3" label="$4"
  log "=== corrected metric on $label ==="
  CHAM_EXPLOIT_BP="$PWD/$out/robust-7/policy" \
  CHAM_EXPLOIT_BUCKETS="$PWD/$buckets" \
  CHAM_EXPLOIT_CONFIG="$PWD/$cfg" \
    cargo nextest run -p cham-blueprint \
      -E 'test(par5m_metric_compare)' \
      --run-ignored all --no-capture 2>&1 \
      | grep -E "clairvoyant|tabular|ratio|^==" \
      > "artifacts/$label-metric.log" || true
  log "  metric:"; cat "artifacts/$label-metric.log"
}

# 1. tiny curve under corrected trainer
for iters in 500000 5000000 20000000; do
  train_tiny "$iters" && \
    metric "artifacts/par-f5-tiny-${iters}" "$CFG" "$BUCKETS" "par-f5-tiny-${iters}"
done

# 2. DCFR proper
train_tiny_dcfr && \
  metric "artifacts/par-f5-tiny-dcfr15" "$CFG" "$BUCKETS" "par-f5-tiny-dcfr15"

# 3. medium under corrected trainer
train_medium && \
  metric "artifacts/par-f5-medium-20M" \
         "config/abstraction-medium.toml" \
         "artifacts/buckets-medium" \
         "par-f5-medium-20M"

log "QUEUE DONE"
