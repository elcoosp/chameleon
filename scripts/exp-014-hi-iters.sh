#!/usr/bin/env bash
# EXP-014 follow-up (v6 runbook item 7): isolate capacity vs coverage.
#
# Retrains the widened-full bundle at 4x iters (2M vs 500k). If fallback
# recovers toward 3.3% (pre-widening full baseline) while KEEPING
# jamfix/pnash fixed, the full-scale regression was under-training, not a
# fundamental coverage tradeoff. If not, it's a per-specialist capacity
# ceiling → 5th specialist is the next step.
set -uo pipefail
cd "$(git rev-parse --show-toplevel)"
LOG=artifacts/exp-014-hi-iters.log
: > "$LOG"
log() { echo "[$(date +%H:%M:%S)] $*" | tee -a "$LOG"; }

ROT=(
  "0|jamfix"
  "1|pnash:overfold:0.05"
  "2|pnash:overfold:0.10"
  "3|arch:lag"
)
ITERS=2000000
SEED=7
CFG=config/abstraction.toml
BUCKETS=artifacts/buckets-full
OUTROOT=artifacts/blueprints-widened-full-hi-iters
AGENT=artifacts/agent-widened-full-hi-iters

mkdir -p "$OUTROOT"
for entry in "${ROT[@]}"; do
  slot="${entry%%|*}"
  opp="${entry##*|}"
  log "  slot $slot <- $opp (iters=$ITERS seed=$SEED)"
  cargo run -q --release -p cham-cli -- train-bp \
    --mode exploit --opponent "$opp" \
    --iters "$ITERS" --depth 100 --seed "$SEED" \
    --config "$CFG" \
    --buckets "$BUCKETS" \
    --out "$OUTROOT/slot$slot" \
    --threads 4 2>&1 | tee -a "$LOG" | tail -3
done
log "=== assemble $AGENT ==="
rm -rf "$AGENT"
mkdir -p "$AGENT/experts/0" "$AGENT/experts/1" "$AGENT/experts/2" "$AGENT/experts/3" "$AGENT/robust"
cp -a "$BUCKETS" "$AGENT/buckets"
cp -a "$CFG" "$AGENT/abstraction.toml"
for entry in "${ROT[@]}"; do
  slot="${entry%%|*}"
  cp -a "$OUTROOT/slot$slot/exploit-$SEED/policy/policy.bin" \
        "$AGENT/experts/$slot/policy.bin"
done
cp -a "artifacts/agent-full/robust/policy.bin" "$AGENT/robust/policy.bin"
log "  robust: reused from artifacts/agent-full/robust/policy.bin"
log "=== done $(date) ==="
log "measure with: DIAG_DEALS=60 probe --diag-fallback --bundle $AGENT --agent full"
