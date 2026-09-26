#!/usr/bin/env bash
# EXP-014 — widened specialist training rotation (v4 brainstorm, R1).
#
# train-bp takes ONE --opponent per invocation, so the widening is expressed
# as: assign each of the 4 expert slots a distinct widened rotation partner
# from config/training/rotation-widened.toml. Robust stays self-play (the
# abstraction is unchanged, so the existing robust artifact is bit-identical
# for the same seed/iters).
#
# Produces artifacts/blueprints-widened-tiny/  and  artifacts/blueprints-widened-full/
# Then assembles artifacts/agent-widened-tiny/ and artifacts/agent-widened-full/
# so the ladder/probe can be pointed at them.
set -uo pipefail
cd "$(git rev-parse --show-toplevel)"
LOG=artifacts/exp-014-run.log
: > "$LOG"
log() { echo "[$(date +%H:%M:%S)] $*" | tee -a "$LOG"; }

# Rotation assignment: (slot_index, opponent_id)
# Uses the widened list from config/training/rotation-widened.toml.
ROT=(
  "0|jamfix"
  "1|pnash:overfold:0.05"
  "2|pnash:overfold:0.10"
  "3|arch:lag"
)
ITERS=500000
SEED=7

run_abstraction() {
  local label="$1" cfg="$2" buckets="$3" outroot="$4"
  log "=== EXP-014 ($label): ${cfg} + ${buckets} → ${outroot} ==="
  mkdir -p "$outroot"
  for entry in "${ROT[@]}"; do
    local slot opp
    slot="${entry%%|*}"
    opp="${entry##*|}"
    log "  slot $slot ← $opp (iters=$ITERS seed=$SEED)"
    cargo run -q --release -p cham-cli -- train-bp \
      --mode exploit --opponent "$opp" \
      --iters "$ITERS" --depth 100 --seed "$SEED" \
      --config "$cfg" \
      --buckets "$buckets" \
      --out "$outroot/slot$slot" \
      --threads 4 2>&1 | tee -a "$LOG" | tail -3
  done
  log "=== EXP-014 ($label): training done; assemble agent bundle ==="
  local agent="artifacts/agent-widened-$label"
  rm -rf "$agent"
  mkdir -p "$agent/experts/0" "$agent/experts/1" "$agent/experts/2" "$agent/experts/3" "$agent/robust"
  cp -a "$buckets" "$agent/buckets"
  cp -a "$cfg" "$agent/abstraction.toml"
  for entry in "${ROT[@]}"; do
    local slot
    slot="${entry%%|*}"
    # train-bp names the run dir "<mode>-<seed>"; for exploit it's exploit-<seed>
    cp -a "$outroot/slot$slot/exploit-$SEED/policy/policy.bin" \
          "$agent/experts/$slot/policy.bin"
  done
  # Robust: reuse the existing artifact from artifacts/agent if present, else
  # train it (deterministic — same seed/iters = same bytes).
  if [ -f artifacts/agent/robust/policy.bin ]; then
    cp -a artifacts/agent/robust/policy.bin "$agent/robust/policy.bin"
    log "  robust: reused from artifacts/agent"
  else
    log "  robust: no existing artifact; training fresh"
    cargo run -q --release -p cham-cli -- train-bp \
      --mode robust --iters "$ITERS" --depth 100 --seed "$SEED" \
      --config "$cfg" --buckets "$buckets" \
      --out "$outroot/robust" --threads 4 2>&1 | tee -a "$LOG" | tail -2
    cp -a "$outroot/robust/robust-$SEED/policy/policy.bin" "$agent/robust/policy.bin"
  fi
  log "  assembled: $agent"
}

# --- tiny first (matches the ladder's evaluation pool) ---
run_abstraction tiny \
  config/abstraction-tiny.toml \
  artifacts/buckets-tiny \
  artifacts/blueprints-widened-tiny

# --- full second (P1's 26.7% target) ---
run_abstraction full \
  config/abstraction.toml \
  artifacts/buckets-full \
  artifacts/blueprints-widened-full

log "=== EXP-014 training done $(date) ==="
log "measure with: probe --diag-fallback --bundle artifacts/agent-widened-tiny  --agent full"
log "              probe --diag-fallback --bundle artifacts/agent-widened-full  --agent full"
