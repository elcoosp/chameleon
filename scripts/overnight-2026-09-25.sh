#!/usr/bin/env bash
# Overnight driver 2026-09-25. Full 10h budget.
#
#   0. wait for turn EHS (already done, skips)
#   1. tiny-robust 10k
#   2. tiny-experts x4
#   3. assemble artifacts/agent
#   4. tiny-ladder (unlock)
#   5. tiny-probe
#   6. FULL FLOP EHS in BACKGROUND (~3.5h GPU)
#      A/B chain in FOREGROUND while flop runs:
#      6a. thread-mode sweep (deterministic/hogwild/snapbatch) @10k
#      6b. seed sweep (3 seeds) @10k robust
#      6c. depth sweep (50/100/200 bb) @10k robust
#      6d. iters sweep (1k/5k/50k) @100bb robust
#   7. wait_bg flop, verify --gpu
#
# Launch:
#   mkdir -p artifacts/overnight-2026-09-25
#   nohup nice -n 10 bash scripts/overnight-2026-09-25.sh \
#     > artifacts/overnight-2026-09-25/nohup.log 2>&1 &

set -uo pipefail
cd "$(git rev-parse --show-toplevel)"
OUT=artifacts/overnight-2026-09-25
mkdir -p "$OUT"
LOG="$OUT/driver.log"; SUMMARY="$OUT/summary.txt"
: > "$LOG"; : > "$SUMMARY"
echo "$$" > "$OUT/driver.pid"

log()  { echo "[$(date +%H:%M:%S)] $*" | tee -a "$LOG"; }
mark() {
  printf "PHASE_%-30s wall_s=%-7s status=%s\n" "$1" "$2" "$3" | tee -a "$SUMMARY"
  if [ -n "${PHASE_JSONL:-}" ]; then
    printf '{"name":"%s","wall_s":%s,"status":"%s","ts":"%s"}\n' "$1" "$2" "$3" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" >> "$PHASE_JSONL"
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
  log "=== phase $name (budget ${budget}s) — $* ==="
  local t0; t0=$(date +%s)
  run_to "$budget" "$@"
  local rc=$?; local dt=$(( $(date +%s) - t0 ))
  local st="ok"; [ "$rc" -ne 0 ] && st="rc=$rc"
  mark "$name" "$dt" "$st"
  log "phase $name done rc=$rc in ${dt}s"
  return $rc
}

declare -A BG_PIDS
phase_bg() {
  local name="$1" budget="$2"; shift 2
  log "=== phase_bg $name (budget ${budget}s) — $* ==="
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
  BG_PIDS[$name]=$!
  log "phase_bg $name pid=${BG_PIDS[$name]}"
}
wait_bg() {
  local name="$1"
  [ -z "${BG_PIDS[$name]:-}" ] && { log "wait_bg $name: no pid"; return 1; }
  wait "${BG_PIDS[$name]}" 2>/dev/null || true
  log "wait_bg $name: done"
}

# Watchdog: RSS > 12 GB or disk < 3 GB → abort.
mem_disk_watch() {
  while true; do
    sleep 30
    local pid rss_kb free_gb
    pid=$(pgrep -P $$ cargo 2>/dev/null | head -1 || true)
    if [ -n "${pid:-}" ]; then
      rss_kb=$(ps -o rss= -p "$pid" 2>/dev/null | tr -d ' ' || echo 0)
      if [ "${rss_kb:-0}" -gt $((12 * 1024 * 1024)) ]; then
        log "MEM GUARD: rss=${rss_kb}KB > 12GB — killing $pid"
        kill -TERM "$pid" 2>/dev/null || true; sleep 5; kill -KILL "$pid" 2>/dev/null || true
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
# ---- run metadata (once) ----
{
  echo "start_utc: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "git_rev: $(git rev-parse HEAD 2>/dev/null || echo unknown)"
  echo "git_branch: $(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo unknown)"
  echo "uname: $(uname -a)"
  echo "host_model: $(sysctl -n hw.model 2>/dev/null || echo unknown)"
  echo "hw_memsize_bytes: $(sysctl -n hw.memsize 2>/dev/null || echo 0)"
  echo "hw_ncpu: $(sysctl -n hw.ncpu 2>/dev/null || echo 0)"
  echo "disk_free_gb_at_start: $(df -g . | tail -1 | awk '{print $4}')"
  echo "cargo_version: $(cargo --version 2>/dev/null || echo unknown)"
  echo "rustc_version: $(rustc --version 2>/dev/null || echo unknown)"
  echo "pwd: $(pwd)"
} > "$OUT/run-metadata.txt"
log "metadata: $OUT/run-metadata.txt"
: > "$OUT/phases.jsonl"
PHASE_JSONL="$OUT/phases.jsonl"

# ============ Phase 0: wait turn (already done) ============
PIDFILE=artifacts/gpu-tables/turn-build.pid
waited=0
while [ -f "$PIDFILE" ] && kill -0 "$(cat "$PIDFILE")" 2>/dev/null; do
  sleep 30; waited=$((waited + 30))
  [ "$waited" -gt 3600 ] && break
done
mark "turn-ehs-wait" "$waited" "done"

# ============ Phases 1-3: tiny agent ============
phase tiny-robust 1800 \
  cargo run -q --release -p cham-cli -- train-bp \
    --mode robust --iters 10000 --depth 100 --seed 7 \
    --out artifacts/blueprints-tiny --threads 4

for opp in nit tag lag station; do
  phase "tiny-expert-$opp" 900 \
    cargo run -q --release -p cham-cli -- train-bp \
      --mode exploit --opponent "arch:$opp" --iters 10000 --depth 100 --seed 7 \
      --out artifacts/blueprints-tiny --threads 4
done

log "assembling artifacts/agent"
rm -rf artifacts/agent
mkdir -p artifacts/agent/experts/{0,1,2,3} artifacts/agent/robust
cp -a artifacts/buckets-tiny artifacts/agent/buckets 2>/dev/null || true
cp -a config/abstraction-tiny.toml artifacts/agent/abstraction.toml 2>/dev/null || true
if [ -f artifacts/blueprints-tiny/robust-7/policy/policy.bin ]; then
  cp -a artifacts/blueprints-tiny/robust-7/policy/policy.bin artifacts/agent/robust/policy.bin
  for i in 0 1 2 3; do
    cp -a artifacts/blueprints-tiny/robust-7/policy/policy.bin "artifacts/agent/experts/$i/policy.bin"
  done
fi
i=0
for opp in nit tag lag station; do
  found=""
  for cand in artifacts/blueprints-tiny/exploit-7-*/policy/policy.bin \
              artifacts/blueprints-tiny/*"$opp"*/policy/policy.bin; do
    [ -f "$cand" ] && found="$cand" && break
  done
  [ -n "$found" ] && cp -a "$found" "artifacts/agent/experts/$i/policy.bin" || true
  i=$((i + 1))
done
log "artifacts/agent layout:"; find artifacts/agent -maxdepth 3 -type f 2>/dev/null | tee -a "$LOG" || true

# ============ Phases 4-5: the unlock ============
phase tiny-ladder 1800 \
  cargo run -q --release -p cham-cli -- ladder --fast --agent full
phase tiny-probe 900 \
  cargo run -q --release -p cham-cli -- probe --agent full

# ============================================================
# A/B: is the ladder output stable run-to-run on the same agent?
# ============================================================
phase ab-ladder-variance 1800 \
  cargo run -q --release -p cham-cli -- ladder --fast --agent full

# ============================================================
# A/B: does the mixture routing actually help vs any single arm?
#   Same tiny bundle; only the --agent value changes.
# ============================================================
for arm in robust-only argmax bayes; do
  phase "ab-arm-$arm" 1800 \
    cargo run -q --release -p cham-cli -- ladder --fast --agent "$arm"
done

for arm in robust-only argmax bayes; do
  phase "ab-probe-$arm" 600 \
    cargo run -q --release -p cham-cli -- probe --agent "$arm"
done

# ============ Phase 6: full flop in BACKGROUND ============
phase_bg full-flop 14400 \
  cargo run -q --release -p cham-gpu --features metal --bin gpu-build -- \
    --kind flop --limit 0 --out artifacts/gpu-tables --sample 40 --batch 512

# ============ A/B chain (foreground, nice -n 15) ============
AB="$OUT/ab"; mkdir -p "$AB"

# 6a. thread-mode sweep
for tm in deterministic hogwild snapbatch; do
  phase "ab-tm-$tm" 1200 \
    nice -n 15 cargo run -q --release -p cham-cli -- train-bp \
      --mode robust --iters 10000 --depth 100 --seed 7 \
      --threads 4 --thread-mode "$tm" \
      --out "$AB/tm-$tm"
done

# 6b. seed variance
for s in 11 22 33; do
  phase "ab-seed-$s" 1200 \
    nice -n 15 cargo run -q --release -p cham-cli -- train-bp \
      --mode robust --iters 10000 --depth 100 --seed "$s" \
      --out "$AB/seed-$s" --threads 4
done

# 6c. depth sweep
for d in 50 100 200; do
  phase "ab-depth-$d" 1200 \
    nice -n 15 cargo run -q --release -p cham-cli -- train-bp \
      --mode robust --iters 10000 --depth "$d" --seed 7 \
      --out "$AB/depth-$d" --threads 4
done

# 6d. iters sweep
for it in 1000 5000 50000; do
  phase "ab-iters-$it" 2400 \
    nice -n 15 cargo run -q --release -p cham-cli -- train-bp \
      --mode robust --iters "$it" --depth 100 --seed 7 \
      --out "$AB/iters-$it" --threads 4
done

# ============ Phase 7: wait for flop, verify ============
wait_bg full-flop
phase verify-gpu 600 \
  cargo run -q --release -p cham-cli -- verify --gpu

# ============ Summary ============
total=$(( $(date +%s) - t0_all ))
log "driver done; total wall ${total}s"
{
  echo
  echo "=== overnight summary ==="
  echo "total wall: ${total}s"
  echo
  echo "--- phase results ---"
  cat "$SUMMARY"
  echo
  echo "--- turn manifest ---"
  python3 -c "import json; m=json.load(open('artifacts/gpu-tables/turn.json')); print(json.dumps({k:m[k] for k in ('kind','boards','blake3','boards_per_s','throughput_evals_per_s','complete')}, indent=2))" 2>/dev/null || echo "(missing)"
  echo
  echo "--- flop manifest ---"
  python3 -c "import json; m=json.load(open('artifacts/gpu-tables/flop.json')); print(json.dumps({k:m[k] for k in ('kind','boards','blake3','boards_per_s','throughput_evals_per_s','complete')}, indent=2))" 2>/dev/null || echo "(missing)"
} >> "$SUMMARY"
kill "$WATCH_PID" 2>/dev/null || true
exit 0
