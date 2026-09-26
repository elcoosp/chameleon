#!/usr/bin/env bash
# Retrain the 5 blueprints against artifacts/buckets-exact (the EMD feature set
# at CPU validation scale), assemble artifacts/agent-exact, then run the bucket
# audit on both bundles and diff the ratios.
set -uo pipefail
cd "$(git rev-parse --show-toplevel)"
LOG=artifacts/exp-emd-retrain.log
: > "$LOG"
log() { echo "[$(date +%H:%M:%S)] $*" | tee -a "$LOG"; }

CFG=config/abstraction-tiny.toml
BUCKETS=artifacts/buckets-exact
OUT=artifacts/blueprints-exact
AGENT=artifacts/agent-exact
ITERS=500000
SEED=7

log "=== EMD retrain: robust ==="
mkdir -p "$OUT"
cargo run -q --release -p cham-cli -- train-bp \
  --mode robust --iters "$ITERS" --depth 100 --seed "$SEED" \
  --config "$CFG" --buckets "$BUCKETS" --out "$OUT" --threads 4 2>&1 | tee -a "$LOG" | tail -2

for opp in nit tag lag station; do
  log "=== EMD retrain: $opp ==="
  cargo run -q --release -p cham-cli -- train-bp \
    --mode exploit --opponent "arch:$opp" \
    --iters "$ITERS" --depth 100 --seed "$SEED" \
    --config "$CFG" --buckets "$BUCKETS" \
    --out "$OUT/$opp" --threads 4 2>&1 | tee -a "$LOG" | tail -2
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

log "=== AUDIT: baseline (tiny buckets) ==="
cargo run -q --release -p cham-cli -- audit-buckets --generate \
  --bundle artifacts/agent --deals 60 --out /tmp/audit-tiny.json 2>&1 | tail -2 | tee -a "$LOG"

log "=== AUDIT: exact buckets ==="
cargo run -q --release -p cham-cli -- audit-buckets --generate \
  --bundle "$AGENT" --deals 60 --out /tmp/audit-exact.json 2>&1 | tail -2 | tee -a "$LOG"

log "=== DONE $(date) ==="
# Extract ratio from the audit tool's human-readable line, not the JSON tail.
tiny_r=$(cargo run -q --release -p cham-cli -- audit-buckets --input /tmp/audit-tiny.json 2>&1 | tail -1)
exact_r=$(cargo run -q --release -p cham-cli -- audit-buckets --input /tmp/audit-exact.json 2>&1 | tail -1)
log "tiny  audit: $tiny_r"
log "exact audit: $exact_r"
