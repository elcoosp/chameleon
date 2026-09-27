#!/usr/bin/env bash
# Overnight 2026-09-27: does robust CFR+ training converge BELOW uniform's LBR
# on the tiny abstraction? Three sub-experiments in priority order:
#
#   A. Convergence sweep on tiny: robust at 10k..10M iters × 3 seeds.
#      Records LBR@depth100 seat-mean per (iters, seed). The crossing (if any)
#      tells us whether the trained BP ever becomes harder to exploit than
#      uniform — the core viability question.
#
#   B. Cross-abstraction reference: benchmark the already-trained agent bundles
#      (artifacts/agent, -exact, -widened-tiny, -full, -widened-full) as the
#      fixed policy, to see whether the mixture or the EMD features help
#      against a BR exploiter.
#
#   C. α/γ top-cell confirmation: retrain the three γ=0.5 cells at 1M iters,
#      3 seeds, to see if the γ=0.5 advantage holds with more training.
#
# Emits JSONL to artifacts/overnight-lbr-<timestamp>/records.jsonl in the
# ledger-compatible shape (run/type/a/b/delta_mb/notes). Idempotent: rerunning
# the driver picks up where it left off (any run whose record already exists
# is skipped).
set -uo pipefail
cd "$(git rev-parse --show-toplevel)"

# L-24 fix (2026-09-27): the docstring above promises idempotency ("any run
# whose record already exists is skipped"), but the code used a fresh
# `$STAMP` per invocation and truncated the records file — so a rerun
# re-executed everything into a new directory and never saw prior runs.
# Fix: ROOT is stable (env-overridable for parallel experiments) and the
# records file is APPEND-ONLY. `record_exists <run-id>` is available for
# the individual run functions to consult before doing expensive work.
ROOT=${OVERNIGHT_ROOT:-artifacts/overnight-lbr}
mkdir -p "$ROOT"
RECORDS="$ROOT/records.jsonl"
LOG="$ROOT/run.log"
# Append-only: do NOT truncate on rerun — that is what broke idempotency.
touch "$RECORDS" "$LOG"
log() { echo "[$(date +%H:%M:%S)] $*" | tee -a "$LOG"; }

# L-24: returns 0 when a record with the given "run" id already exists in
# the ledger. Individual experiment functions call this before their heavy
# work and `return 0` early on a hit.
record_exists() {
  local rid="$1"
  grep -q "\"run\":\"$rid\"" "$RECORDS" 2>/dev/null
}

CFG=config/abstraction-tiny.toml
BUCKETS=artifacts/buckets-tiny
FCFG=config/abstraction.toml
FBUCKETS=artifacts/buckets-full
DEALS=${OVERNIGHT_DEALS:-200}
THREADS=2

# ---- helper: run one bench of a policy file, extract the depth-100 seat values
bench_policy() {
  local label="$1" bpdir="$2" cfg="$3" buckets="$4" seed="$5"
  if [ ! -d "$bpdir" ]; then
    log "  SKIP bench $label: $bpdir missing"
    return 1
  fi
  log "  bench $label (bp=$bpdir, seed=$seed, deals=$DEALS)"
  CHAM_EXPLOIT_BP="$bpdir" \
  CHAM_EXPLOIT_BUCKETS="$buckets" \
  CHAM_EXPLOIT_CONFIG="$cfg" \
  CHAM_EXPLOIT_DEALS="$DEALS" \
  CHAM_EXPLOIT_SEED="$seed" \
    cargo bench -q -p cham-blueprint --bench exploitability 2>&1 \
    | grep -E 'exploitability\[bp depth=100' | tee -a "$LOG"
}

# ---- helper: append a JSONL record
record() {
  local run="$1" note="$2" s0="$3" s1="$4" extra="${5:-}"
  python3 - "$run" "$note" "$s0" "$s1" "$extra" "$RECORDS" <<'PY'
import json, sys, time
run, note, s0, s1, extra, path = sys.argv[1:7]
entry = {
    "ts": int(time.time()),
    "run": run,
    "type": "bench",
    "a": {"metric": "LBR@depth100", "deals": int(__import__("os").environ.get("OVERNIGHT_DEALS", "200"))},
    "delta_mb": None,
    "ci": None,
    "sprt": None,
    "promote": False,
    "seatings": 0,
    "notes": note,
}
try:
    entry["a"]["seat0_lbr"] = float(s0)
    entry["a"]["seat1_lbr"] = float(s1)
    entry["a"]["seat_mean_lbr"] = (float(s0) + float(s1)) / 2
except (ValueError, TypeError):
    pass
if extra:
    try:
        entry["a"].update(json.loads(extra))
    except Exception:
        pass
with open(path, "a") as f:
    f.write(json.dumps(entry, separators=(",", ":")) + "\n")
PY
}

# ---- A. Convergence sweep on tiny ---------------------------------------
log "=== A. convergence sweep on tiny ==="
log "  (baseline uniform reference, from prior bench runs: ~40413 seat0 / ~35099 seat1)"
for iters in 10000 30000 100000 300000 1000000 3000000 10000000; do
  for seed in 7 11 13; do
    RUNDIR="$ROOT/convergence/tiny-i${iters}-s${seed}"
    if [ -f "$RUNDIR/robust-$seed/policy/policy.bin" ]; then
      log "  reuse $RUNDIR"
    else
      log "  train tiny i=$iters s=$seed"
      nice -n 10 cargo run -q --release -p cham-cli -- train-bp \
        --mode robust --iters "$iters" --depth 100 --seed "$seed" \
        --config "$CFG" --buckets "$BUCKETS" --out "$RUNDIR" \
        --threads "$THREADS" 2>&1 | tee -a "$LOG" | tail -1
    fi
    out=$(bench_policy "tiny i=$iters s=$seed" \
          "$RUNDIR/robust-$seed/policy" "$CFG" "$BUCKETS" "0xE8")
    s0=$(echo "$out" | grep 'depth=100 seat=0' | sed -n 's/.*lbr \([+-][0-9.]*\).*/\1/p')
    s1=$(echo "$out" | grep 'depth=100 seat=1' | sed -n 's/.*lbr \([+-][0-9.]*\).*/\1/p')
    record "conv-tiny-i${iters}-s${seed}" \
      "overnight-lbr-convergence A: robust at $iters iters, seed $seed, tiny abstraction, bench deals=$DEALS" \
      "$s0" "$s1"
  done
done

# ---- B. cross-abstraction reference (bench existing bundles) ------------
log "=== B. cross-abstraction reference ==="
declare -a BUNDLES=(
  "agent-tiny:artifacts/agent/robust/policy:config/abstraction-tiny.toml:artifacts/buckets-tiny"
  "agent-exact:artifacts/agent-exact/robust/policy:config/abstraction-tiny.toml:artifacts/buckets-exact"
  "agent-widened-tiny:artifacts/agent-widened-tiny/robust/policy:config/abstraction-tiny.toml:artifacts/buckets-tiny"
  "agent-full:artifacts/agent-full/robust/policy:config/abstraction.toml:artifacts/buckets-full"
  "agent-widened-full:artifacts/agent-widened-full/robust/policy:config/abstraction.toml:artifacts/buckets-full"
)
for entry in "${BUNDLES[@]}"; do
  name=$(echo "$entry" | cut -d: -f1)
  bp=$(echo "$entry" | cut -d: -f2)
  cfg=$(echo "$entry" | cut -d: -f3)
  bk=$(echo "$entry" | cut -d: -f4)
  out=$(bench_policy "cross $name" "$bp" "$cfg" "$bk" "0xE8")
  s0=$(echo "$out" | grep 'depth=100 seat=0' | sed -n 's/.*lbr \([+-][0-9.]*\).*/\1/p')
  s1=$(echo "$out" | grep 'depth=100 seat=1' | sed -n 's/.*lbr \([+-][0-9.]*\).*/\1/p')
  record "cross-${name}" \
    "overnight-lbr-convergence B: bench pre-trained robust in $name (bp=$bp)" \
    "$s0" "$s1"
done

# ---- C. α/γ top-cell confirmation (γ=0.5, 1M iters, 3 seeds) ------------
log "=== C. α/γ top-cell confirmation ==="
for alpha in 0.5 0.9 1.0; do
  gamma=0.5
  for seed in 7 11 13; do
    RUNDIR="$ROOT/ag/tiny-a${alpha}-g${gamma}-s${seed}"
    if [ -f "$RUNDIR/robust-$seed/policy/policy.bin" ]; then
      log "  reuse $RUNDIR"
    else
      log "  train α=$alpha γ=$gamma s=$seed"
      nice -n 10 cargo run -q --release -p cham-cli -- train-bp \
        --mode robust --iters 1000000 --depth 100 --seed "$seed" \
        --config "$CFG" --buckets "$BUCKETS" \
        --regret-discount "$alpha" --avg-gamma "$gamma" \
        --out "$RUNDIR" --threads "$THREADS" 2>&1 | tee -a "$LOG" | tail -1
    fi
    out=$(bench_policy "ag a=$alpha g=$gamma s=$seed" \
          "$RUNDIR/robust-$seed/policy" "$CFG" "$BUCKETS" "0xE8")
    s0=$(echo "$out" | grep 'depth=100 seat=0' | sed -n 's/.*lbr \([+-][0-9.]*\).*/\1/p')
    s1=$(echo "$out" | grep 'depth=100 seat=1' | sed -n 's/.*lbr \([+-][0-9.]*\).*/\1/p')
    record "ag-tiny-a${alpha}-g${gamma}-s${seed}" \
      "overnight-lbr-convergence C: α=$alpha γ=$gamma at 1M iters, seed $seed, tiny" \
      "$s0" "$s1"
  done
done

log "=== DONE $(date) ==="
log "records: $RECORDS ($(wc -l < "$RECORDS") entries)"
log "to consume after the run: python3 -c 'import json; [print(json.loads(l)[\"run\"]) for l in open(\"$RECORDS\")]'"
