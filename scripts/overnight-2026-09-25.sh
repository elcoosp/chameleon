#!/usr/bin/env bash
# Overnight driver 2026-09-25 — Plan B:
#   1. Wait for the running turn EHS build to complete
#   2. Build a tiny agent bundle (train robust + 4 experts, assemble)
#   3. Run the first real ladder + probe — the ±0.0 unlock
#   4. Full flop EHS build (GPU-free for ~2-3 h while you sleep)
#   5. verify --gpu against both tables
#   6. Print a summary
#
# Launch:
#   nohup nice -n 10 bash scripts/overnight-2026-09-25.sh \
#     > artifacts/overnight-2026-09-25/nohup.log 2>&1 &
# Monitor:
#   tail -f artifacts/overnight-2026-09-25/driver.log
# Stop:
#   kill $(cat artifacts/overnight-2026-09-25/driver.pid)
#
# NOT in this plan: full-buckets (sample_orbits: 0 enumerates 56 M orbits —
# multi-day, not one night) and --full ladder (25k deals × 9 opponents).
set -uo pipefail
cd "$(git rev-parse --show-toplevel)"
OUT=artifacts/overnight-2026-09-25
mkdir -p "$OUT"
LOG="$OUT/driver.log"
SUMMARY="$OUT/summary.txt"
: > "$LOG"
: > "$SUMMARY"
echo "$$" > "$OUT/driver.pid"

log()  { echo "[$(date +%H:%M:%S)] $*" | tee -a "$LOG"; }
mark() { printf "PHASE_%-16s wall_s=%-7s status=%s\n" "$1" "$2" "$3" | tee -a "$SUMMARY"; }
t0_all=$(date +%s)

# Portable timeout via a kill-after-N background watcher.
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

mem_disk_watch() {
  while true; do
    sleep 30
    local pid rss_kb free_gb
    pid=$(pgrep -P $$ cargo 2>/dev/null | head -1 || true)
    if [ -n "${pid:-}" ]; then
      rss_kb=$(ps -o rss= -p "$pid" 2>/dev/null | tr -d ' ' || echo 0)
      if [ "${rss_kb:-0}" -gt $((12 * 1024 * 1024)) ]; then
        log "MEMORY GUARD: rss=${rss_kb}KB > 12GB — killing $pid"
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
mem_disk_watch &
WATCH_PID=$!
trap 'kill "$WATCH_PID" 2>/dev/null; exit 0' INT TERM

phase() {
  local name="$1" budget="$2"; shift 2
  CURRENT_PHASE="$name"
  log "=== phase $name: budget ${budget}s — $* ==="
  local t0=$(date +%s)
  run_to "$budget" "$@"
  local rc=$?
  local dt=$(( $(date +%s) - t0 ))
  local st="ok"; [ "$rc" -ne 0 ] && st="rc=$rc"
  mark "$name" "$dt" "$st"
  log "phase $name done rc=$rc in ${dt}s"
  return $rc
}

# ============================================================
# Phase 0 — wait for the running turn EHS build
# ============================================================
log "overnight driver starting (pid $$)"
PIDFILE=artifacts/gpu-tables/turn-build.pid
TLOG=artifacts/gpu-tables/turn-build.log
waited=0
while [ -f "$PIDFILE" ] && kill -0 "$(cat "$PIDFILE")" 2>/dev/null; do
  sleep 30
  waited=$((waited + 30))
  if [ "$waited" -gt 3600 ]; then
    log "turn build wait exceeded 1 h — continuing anyway"
    break
  fi
done
mark "turn-ehs-wait" "$waited" "done"
log "turn build done; turn.bin present: $(ls -la artifacts/gpu-tables/turn.bin 2>/dev/null | awk '{print $5}')"

# ============================================================
# Phase 1 — tiny robust blueprint
# ============================================================
phase tiny-robust 1800 \
  cargo run -q --release -p cham-cli -- train-bp \
    --mode robust --iters 10000 --depth 100 --seed 7 \
    --out artifacts/blueprints-tiny --threads 4

# ============================================================
# Phase 2 — 4 tiny experts
# ============================================================
for opp in nit tag lag station; do
  phase "tiny-expert-$opp" 900 \
    cargo run -q --release -p cham-cli -- train-bp \
      --mode exploit --opponent "arch:$opp" --iters 10000 --depth 100 --seed 7 \
      --out artifacts/blueprints-tiny --threads 4
done

# ============================================================
# Phase 3 — assemble artifacts/agent from tiny artifacts
# ============================================================
log "assembling artifacts/agent"
rm -rf artifacts/agent
mkdir -p artifacts/agent/experts/0 artifacts/agent/experts/1 artifacts/agent/experts/2 artifacts/agent/experts/3 artifacts/agent/robust
# buckets + abstraction
cp -a artifacts/buckets-tiny artifacts/agent/buckets 2>/dev/null || true
cp -a config/abstraction-tiny.toml artifacts/agent/abstraction.toml 2>/dev/null \
  || cp -a config/abstraction.toml artifacts/agent/abstraction.toml 2>/dev/null || true
# robust expert (also fills missing experts so loader doesn't refuse)
# train-bp writes {out}/{mode}-{seed}/policy/policy.bin
if [ -f artifacts/blueprints-tiny/robust-7/policy/policy.bin ]; then
  cp -a artifacts/blueprints-tiny/robust-7/policy/policy.bin artifacts/agent/robust/policy.bin
  for i in 0 1 2 3; do
    cp -a artifacts/blueprints-tiny/robust-7/policy/policy.bin "artifacts/agent/experts/$i/policy.bin"
  done
fi
# per-opponent experts overwrite when present (paths may carry mode+seed)
i=0
for opp in nit tag lag station; do
  found=""
  # Look for exploit-mode output for this opponent: {out}/{mode}-{seed}/policy/policy.bin
  # train-bp's exploit modes use --opponent; the folder name is not opponent-specific
  # today, so we glob all exploit-* dirs and pick by mtime, but only the newest
  # for this opponent run. Fallback: leave robust-initialised policy.
  for cand in artifacts/blueprints-tiny/exploit-7-*/policy/policy.bin               artifacts/blueprints-tiny/*${opp}*/policy/policy.bin ; do
    [ -f "$cand" ] && found="$cand" && break
  done
  [ -n "$found" ] && cp -a "$found" "artifacts/agent/experts/$i/policy.bin" || true
  i=$((i + 1))
done
log "artifacts/agent layout:"; find artifacts/agent -maxdepth 3 -type f 2>/dev/null | tee -a "$LOG"

# ============================================================
# Phase 4 — tiny ladder (the unlock)
# ============================================================
phase tiny-ladder 1800 \
  cargo run -q --release -p cham-cli -- ladder --fast --agent full

# ============================================================
# Phase 5 — tiny probe (LBR proxy)
# ============================================================
phase tiny-probe 900 \
  cargo run -q --release -p cham-cli -- probe --agent full

# ============================================================
# Phase 6 — full flop EHS build (now the GPU is free)
# ============================================================
phase full-flop 14400 \
  cargo run -q --release -p cham-gpu --features metal --bin gpu-build -- \
    --kind flop --limit 0 --out artifacts/gpu-tables --sample 40 --batch 512

# ============================================================
# Phase 7 — verify --gpu (reads turn + flop)
# ============================================================
phase verify-gpu 600 \
  cargo run -q --release -p cham-cli -- verify --gpu

# ============================================================
# Summary
# ============================================================
total=$(( $(date +%s) - t0_all ))
log "overnight driver done; total wall ${total}s"
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
} >> "$OUT/summary.txt"
kill "$WATCH_PID" 2>/dev/null || true
exit 0
