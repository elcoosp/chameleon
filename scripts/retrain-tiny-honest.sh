#!/usr/bin/env bash
# Retrain the tiny agent (robust + 4 experts) with the RBP-gate fix in place.
# Assembles artifacts/agent-honest/, then runs a probe + LBR bench on it.
# Same params as scripts/run-full-agent.sh: 500k iters, seed 7, threads 4.
# Runs at nice 15 to coexist with pkr-trainer.
set -uo pipefail
cd "$(git rev-parse --show-toplevel)"
LOG=artifacts/retrain-honest.log
: > "$LOG"
log(){ echo "[$(date +%H:%M:%S)] $*" | tee -a "$LOG"; }

CFG=config/abstraction-tiny.toml
BUCKETS=artifacts/buckets-tiny
OUT=artifacts/blueprints-tiny-honest
AGENT=artifacts/agent-honest
ITERS=500000
SEED=7
THREADS=4

log "=== retrain: robust ==="
mkdir -p "$OUT"
nice -n 15 cargo run -q --release -p cham-cli -- train-bp \
  --mode robust --iters "$ITERS" --depth 100 --seed "$SEED" \
  --config "$CFG" --buckets "$BUCKETS" --out "$OUT" --threads "$THREADS" 2>&1 \
  | tee -a "$LOG" | tail -1

for opp in nit tag lag station; do
  log "=== retrain: $opp ==="
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

log "=== probe --diag-fallback on the honest agent ==="
DIAG_DEALS=40 cargo run -q --release -p cham-cli -- probe --diag-fallback \
  --bundle "$AGENT" --agent full 2>&1 | tee -a "$LOG"

log "=== LBR bench on the honest robust ==="
CHAM_EXPLOIT_BP="$PWD/$AGENT/robust" \
CHAM_EXPLOIT_BUCKETS="$PWD/$BUCKETS" \
CHAM_EXPLOIT_CONFIG="$PWD/$CFG" \
CHAM_EXPLOIT_DEALS=200 \
  cargo bench -q -p cham-blueprint --bench exploitability 2>&1 \
  | grep -E 'exploitability\[bp depth=100' | tee -a "$LOG"

log "=== DONE $(date) ==="
