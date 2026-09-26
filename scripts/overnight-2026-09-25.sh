#!/usr/bin/env bash
# Overnight driver 2026-09-25. macOS-safe: no associative arrays.
set -uo pipefail
cd "$(git rev-parse --show-toplevel)"
OUT=artifacts/overnight-2026-09-25
mkdir -p "$OUT"
LOG="$OUT/driver.log"; SUMMARY="$OUT/summary.txt"
: > "$LOG"; : > "$SUMMARY"
# pidfile FIRST thing
echo "$$" > "$OUT/driver.pid"

log() { echo "[$(date +%H:%M:%S)] $*" | tee -a "$LOG"; }
mark() {
  printf "PHASE_%-30s wall_s=%-7s status=%s\n" "$1" "$2" "$3" | tee -a "$SUMMARY"
  if [ -n "${PHASE_JSONL:-}" ]; then
    printf '{"name":"%s","wall_s":%s,"status":"%s","ts":"%s"}\n' \
      "$1" "$2" "$3" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" >> "$PHASE_JSONL"
  fi
}
t0_all=$(date +%s)

run_to() {
  local budget="$1"; shift
  "$@" > "$OUT/$CURRENT_PHASE.log" 2>&1 &
  local pid=$!
  ( sleep "$budget"; kill -TERM "$pid" 2>/dev/null; sleep 10; kill -KILL "$pid" 2>/dev/null ) &
  local w=$!
  wait "$pid"; local rc=$?
  kill "$w" 2>/dev/null || true
  wait "$w" 2>/dev/null || true
  return $rc
}

phase() {
  local name="$1" budget="$2"; shift 2
  CURRENT_PHASE="$name"
  log "=== phase $name (budget ${budget}s) ==="
  local t0; t0=$(date +%s)
  run_to "$budget" "$@"
  local rc=$?; local dt=$(( $(date +%s) - t0 ))
  local st="ok"; [ "$rc" -ne 0 ] && st="rc=$rc"
  mark "$name" "$dt" "$st"
  log "phase $name done rc=$rc in ${dt}s"
  return $rc
}

# Single background phase only (flop). No associative arrays.
BG_PID=""
BG_NAME=""
phase_bg() {
  local name="$1" budget="$2"; shift 2
  BG_NAME="$name"
  log "=== phase_bg $name (budget ${budget}s) ==="
  (
    local t0; t0=$(date +%s)
    "$@" > "$OUT/$name.log" 2>&1 &
    local pid=$!
    ( sleep "$budget"; kill -TERM "$pid" 2>/dev/null; sleep 10; kill -KILL "$pid" 2>/dev/null ) &
    local w=$!
    wait "$pid"; local rc=$?
    kill "$w" 2>/dev/null || true
    wait "$w" 2>/dev/null || true
    local dt=$(( $(date +%s) - t0 ))
    local st="ok"; [ "$rc" -ne 0 ] && st="rc=$rc"
    mark "$name" "$dt" "$st"
    log "phase_bg $name done rc=$rc in ${dt}s"
    echo "$rc" > "$OUT/$name.rc"
  ) &
  BG_PID=$!
  log "phase_bg $name pid=$BG_PID"
}
wait_bg() {
  [ -z "${BG_PID:-}" ] && { log "wait_bg: no bg pid"; return 1; }
  wait "$BG_PID" 2>/dev/null || true
  log "wait_bg $BG_NAME: done"
  BG_PID=""
}

mem_disk_watch() {
  while true; do
    sleep 30
    local pid rss_kb free_gb
    pid=$(pgrep -P $$ cargo 2>/dev/null | head -1 || true)
    if [ -n "${pid:-}" ]; then
      rss_kb=$(ps -o rss= -p "$pid" 2>/dev/null | tr -d ' ' || echo 0)
      if [ "${rss_kb:-0}" -gt $((12 * 1024 * 1024)) ]; then
        log "MEM GUARD: rss=${rss_kb}KB > 12GB — killing $pid"
        kill -TERM "$pid" 2>/dev/null || true
        sleep 5
        kill -KILL "$pid" 2>/dev/null || true
      fi
    fi
    free_gb=$(df -g . | tail -1 | awk '{print $4}' 2>/dev/null || echo 99)
    if [ "${free_gb:-99}" -lt 3 ]; then
      log "DISK GUARD: free=${free_gb}GB < 3GB — aborting"
      kill "$$" 2>/dev/null || true
    fi
  done
}
mem_disk_watch & WATCH_PID=$!
trap 'kill "$WATCH_PID" 2>/dev/null; exit 0' INT TERM

log "overnight driver starting (pid $$)"

# metadata
{
  echo "start_utc: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "git_rev: $(git rev-parse HEAD 2>/dev/null || echo unknown)"
  echo "git_branch: $(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo unknown)"
  echo "uname: $(uname -a)"
  echo "host_model: $(sysctl -n hw.model 2>/dev/null || echo unknown)"
  echo "hw_memsize_bytes: $(sysctl -n hw.memsize 2>/dev/null || echo 0)"
  echo "hw_ncpu: $(sysctl -n hw.ncpu 2>/dev/null || echo 0)"
  echo "disk_free_gb_at_start: $(df -g . | tail -1 | awk '{print $4}')"
  echo "bash_version: $(bash --version | head -1)"
  echo "cargo_version: $(cargo --version 2>/dev/null || echo unknown)"
  echo "rustc_version: $(rustc --version 2>/dev/null || echo unknown)"
  echo "pwd: $(pwd)"
} > "$OUT/run-metadata.txt"
log "metadata: $OUT/run-metadata.txt"
: > "$OUT/phases.jsonl"
PHASE_JSONL="$OUT/phases.jsonl"

# Phase 0 — turn wait (already done)
PIDFILE=artifacts/gpu-tables/turn-build.pid
waited=0
while [ -f "$PIDFILE" ] && kill -0 "$(cat "$PIDFILE")" 2>/dev/null; do
  sleep 30; waited=$((waited + 30))
  [ "$waited" -gt 3600 ] && break
done
mark "turn-ehs-wait" "$waited" "done"

# Phase 1 — tiny robust
phase tiny-robust 1800 \
  cargo run -q --release -p cham-cli -- train-bp \
    --mode robust --iters 500000 --depth 100 --seed 7 \
    --out artifacts/blueprints-tiny --threads 4

# Phase 2 — 4 tiny experts, each into its own subdir
for opp in nit tag lag station; do
  phase "tiny-expert-$opp" 900 \
    cargo run -q --release -p cham-cli -- train-bp \
      --mode exploit --opponent "arch:$opp" --iters 500000 --depth 100 --seed 7 \
      --out "artifacts/blueprints-tiny/$opp" --threads 4
done

# Phase 3 — assemble
log "assembling artifacts/agent"
rm -rf artifacts/agent
mkdir -p artifacts/agent/experts/0 artifacts/agent/experts/1 \
         artifacts/agent/experts/2 artifacts/agent/experts/3 \
         artifacts/agent/robust
cp -a artifacts/buckets-tiny artifacts/agent/buckets 2>/dev/null || true
cp -a config/abstraction-tiny.toml artifacts/agent/abstraction.toml 2>/dev/null || true

ROBUST=artifacts/blueprints-tiny/robust-7/policy/policy.bin
if [ -f "$ROBUST" ]; then
  cp -a "$ROBUST" artifacts/agent/robust/policy.bin
  for i in 0 1 2 3; do
    cp -a "$ROBUST" "artifacts/agent/experts/$i/policy.bin"
  done
fi
i=0
for opp in nit tag lag station; do
  EXP="artifacts/blueprints-tiny/$opp/exploit-7/policy/policy.bin"
  if [ -f "$EXP" ]; then
    cp -a "$EXP" "artifacts/agent/experts/$i/policy.bin"
  fi
  i=$((i + 1))
done
log "artifacts/agent layout:"
find artifacts/agent -maxdepth 3 -type f 2>/dev/null | tee -a "$LOG" || true

# Phases 4-5
phase tiny-ladder 1800 \
  cargo run -q --release -p cham-cli -- ladder --fast --agent full
phase tiny-probe 900 \
  cargo run -q --release -p cham-cli -- probe --agent full

# A/B ladder variance
phase ab-ladder-variance 1800 \
  cargo run -q --release -p cham-cli -- ladder --fast --agent full

# A/B arms
for arm in robust-only argmax bayes; do
  phase "ab-arm-$arm" 1800 \
    cargo run -q --release -p cham-cli -- ladder --fast --agent "$arm"
done
for arm in robust-only argmax bayes; do
  phase "ab-probe-$arm" 600 \
    cargo run -q --release -p cham-cli -- probe --agent "$arm"
done

# Phase 6 — background flop
phase_bg full-flop 14400 \
  cargo run -q --release -p cham-gpu --features metal --bin gpu-build -- \
    --kind flop --limit 0 --out artifacts/gpu-tables --sample 40 --batch 512

# A/B chain while flop runs
AB="$OUT/ab"; mkdir -p "$AB"

for tm in deterministic hogwild snapbatch; do
  phase "ab-tm-$tm" 2400 \
    nice -n 15 cargo run -q --release -p cham-cli -- train-bp \
      --mode robust --iters 100000 --depth 100 --seed 7 \
      --threads 4 --thread-mode "$tm" \
      --out "$AB/tm-$tm"
done

for s in 11 22 33; do
  phase "ab-seed-$s" 2400 \
    nice -n 15 cargo run -q --release -p cham-cli -- train-bp \
      --mode robust --iters 100000 --depth 100 --seed "$s" \
      --out "$AB/seed-$s" --threads 4
done

for d in 50 100 200; do
  phase "ab-depth-$d" 2400 \
    nice -n 15 cargo run -q --release -p cham-cli -- train-bp \
      --mode robust --iters 100000 --depth "$d" --seed 7 \
      --out "$AB/depth-$d" --threads 4
done

for it in 10000 100000 1000000; do
  phase "ab-iters-$it" 7200 \
    nice -n 15 cargo run -q --release -p cham-cli -- train-bp \
      --mode robust --iters "$it" --depth 100 --seed 7 \
      --out "$AB/iters-$it" --threads 4
done

wait_bg

phase verify-gpu 600 \
  cargo run -q --release -p cham-cli -- verify --gpu

# Summary
total=$(( $(date +%s) - t0_all ))
log "driver done; total wall ${total}s"
{
  echo
  echo "=== overnight summary ==="
  echo "total wall: ${total}s"
  echo
  echo "--- run metadata ---"
  cat "$OUT/run-metadata.txt" 2>/dev/null || true
  echo
  echo "--- per-phase JSONL ---"
  cat "$OUT/phases.jsonl" 2>/dev/null || true
  echo
  echo "--- turn manifest ---"
  python3 -c "import json; m=json.load(open('artifacts/gpu-tables/turn.json')); print(json.dumps({k:m[k] for k in ('kind','boards','blake3','boards_per_s','throughput_evals_per_s','complete')}, indent=2))" 2>/dev/null || echo "(missing)"
  echo
  echo "--- flop manifest ---"
  python3 -c "import json; m=json.load(open('artifacts/gpu-tables/flop.json')); print(json.dumps({k:m[k] for k in ('kind','boards','blake3','boards_per_s','throughput_evals_per_s','complete')}, indent=2))" 2>/dev/null || echo "(missing)"
} >> "$OUT/final-report.txt"
kill "$WATCH_PID" 2>/dev/null || true
exit 0
