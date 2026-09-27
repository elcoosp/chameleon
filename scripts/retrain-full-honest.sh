#!/usr/bin/env bash
# Retrain the FULL-abstraction agent (robust + 4 experts) with the RBP-gate
# fix in place. Same params as scripts/run-full-agent.sh (500k iters, seed 7,
# threads 4), same buckets/abstraction, output separated from the stale
# artifacts/agent-full/ so nothing gets confused.
#
# Timing estimate with pruning genuinely off: ~4x the pre-fix full-abstraction
# times, i.e. ~2.5-3.5 hours for all five blueprints (each ~30-50 min).
set -uo pipefail
cd "$(git rev-parse --show-toplevel)"
LOG=artifacts/retrain-full-honest.log
: > "$LOG"
log(){ echo "[$(date +%H:%M:%S)] $*" | tee -a "$LOG"; }

CFG=config/abstraction.toml
BUCKETS=artifacts/buckets-full
OUT=artifacts/blueprints-full-honest
AGENT=artifacts/agent-full-honest
ITERS=500000
SEED=7
THREADS=4

log "=== full-abstraction honest retrain (RBP gate fixed) ==="
log "  config=$CFG buckets=$BUCKETS iters=$ITERS seed=$SEED threads=$THREADS"
mkdir -p "$OUT"

log "=== robust ==="
nice -n 15 cargo run -q --release -p cham-cli -- train-bp \
  --mode robust --iters "$ITERS" --depth 100 --seed "$SEED" \
  --config "$CFG" --buckets "$BUCKETS" --out "$OUT" --threads "$THREADS" 2>&1 \
  | tee -a "$LOG" | tail -1

for opp in nit tag lag station; do
  log "=== $opp ==="
  nice -n 15 cargo run -q --release -p cham-cli -- train-bp \
    --mode exploit --opponent "arch:$opp" \
    --iters "$ITERS" --depth 100 --seed "$SEED" \
    --config "$CFG" --buckets "$BUCKETS" \
    --out "$OUT/$opp" --threads "$THREADS" 2>&1 \
    | tee -a "$LOG" | tail -1
done

log "=== assemble $AGENT ==="
rm -rf "$AGENT"
mkdir -p "$AGENT/experts/0" "$AGENT/experts/1" "$AGENT/experts/2" "$AGENT/experts/3" "$AGENT/robust"
cp -a "$BUCKETS" "$AGENT/buckets"
cp -a "$CFG" "$AGENT/abstraction.toml"
cp -a "$OUT/robust-$SEED/policy/policy.bin" "$AGENT/robust/policy.bin"
i=0
for opp in nit tag lag station; do
  cp -a "$OUT/$opp/exploit-$SEED/policy/policy.bin" "$AGENT/experts/$i/policy.bin"
  i=$((i+1))
done
log "  assembled: $AGENT"

log "=== LBR on the honest full robust ==="
CHAM_EXPLOIT_BP="$PWD/$AGENT/robust" \
CHAM_EXPLOIT_BUCKETS="$PWD/$BUCKETS" \
CHAM_EXPLOIT_CONFIG="$PWD/$CFG" \
CHAM_EXPLOIT_DEALS=200 \
  cargo bench -q -p cham-blueprint --bench exploitability 2>&1 \
  | grep -E 'exploitability\[bp depth=100' | tee -a "$LOG"

log "=== probe --diag-fallback on the honest full agent ==="
DIAG_DEALS=60 cargo run -q --release -p cham-cli -- probe --diag-fallback \
  --bundle "$AGENT" --agent full 2>&1 | tee -a "$LOG"

log "=== DONE $(date) ==="
