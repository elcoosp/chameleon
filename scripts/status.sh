#!/usr/bin/env bash
# One-shot project status. Prints everything relevant on one screen.
# Usage: scripts/status.sh
set -uo pipefail
cd "$(git rev-parse --show-toplevel)"

# --- buckets ---
echo "=== buckets (full abstraction) ==="
if pgrep -f "target/release/chameleon train-buckets" >/dev/null; then
  W=$(pgrep -f "target/release/chameleon train-buckets" | head -1)
  echo "  RUNNING pid=$W  ($(ps -o etime= -p "$W" | tr -d ' '), $(ps -o pcpu= -p "$W" | tr -d ' ') %cpu)"
  echo "  log: $(tail -1 artifacts/full-buckets.log 2>/dev/null)"
else
  if [ -f artifacts/buckets-full/turn.bin ] && [ -f artifacts/buckets-full/meta.json ]; then
    echo "  DONE — $(du -sh artifacts/buckets-full | cut -f1)"
  else
    echo "  not running, no artifacts"
  fi
fi

# --- full-agent queue ---
echo
echo "=== full-agent queue ==="
if pgrep -f "run-full-agent.sh" >/dev/null; then
  echo "  watcher alive ($(ps -o etime= -p "$(pgrep -f run-full-agent.sh | head -1)" | tr -d ' '))"
  tail -3 artifacts/full-agent.log 2>/dev/null | sed 's/^/  /'
else
  echo "  watcher not running"
  if [ -f artifacts/full-agent.log ]; then
    tail -5 artifacts/full-agent.log | sed 's/^/  /'
  fi
fi

# --- any train-bp in flight? ---
if pgrep -f "train-bp" >/dev/null; then
  echo "  train-bp RUNNING: $(pgrep -fl 'train-bp' | head -1)"
fi

# --- gpu tables ---
echo
echo "=== gpu tables ==="
for k in turn flop; do
  if [ -f "artifacts/gpu-tables/$k.json" ]; then
    python3 -c "
import json
m = json.load(open('artifacts/gpu-tables/$k.json'))
print(f'  $k: {m[\"boards\"]} boards, complete={m.get(\"complete\")}, blake3={m[\"blake3\"][:16]}')
" 2>/dev/null || echo "  $k: manifest present, unreadable"
  else
    echo "  $k: no manifest"
  fi
done

# --- git ---
echo
echo "=== git ==="
git log --oneline -5 | sed 's/^/  /'
DIRTY=$(git status --porcelain | wc -l | tr -d ' ')
echo "  working tree: $DIRTY dirty files"
if [ "$DIRTY" -gt 0 ]; then
  git status --short | head -8 | sed 's/^/    /'
fi

# --- machine ---
echo
echo "=== machine ==="
echo "  load: $(uptime | sed 's/.*load averages*: //')"
echo "  disk: $(df -h . | tail -1 | awk '{print $4 " free"}')"
echo "  cargo builds running: $(pgrep -c cargo 2>/dev/null || echo 0)"
