#!/usr/bin/env bash
set -uo pipefail
cd /Users/adm/Documents/Repos/chameleon
mkdir -p artifacts/overnight-2026-09-25
rm -f artifacts/overnight-2026-09-25/driver.pid artifacts/overnight-2026-09-25/launcher.pid
nohup nice -n 10 bash scripts/overnight-2026-09-25.sh \
  > artifacts/overnight-2026-09-25/nohup.log 2>&1 &
PID=$!
echo "$PID" > artifacts/overnight-2026-09-25/launcher.pid
echo "launched pid=$PID"
sleep 3
if kill -0 "$PID" 2>/dev/null; then
  echo "RUNNING"
  echo "--- driver.log head ---"
  head -20 artifacts/overnight-2026-09-25/driver.log 2>/dev/null || true
  echo "--- driver.pid ---"
  cat artifacts/overnight-2026-09-25/driver.pid 2>/dev/null || echo "(not yet)"
else
  echo "DIED"
  echo "--- nohup.log ---"
  cat artifacts/overnight-2026-09-25/nohup.log 2>/dev/null || true
  exit 1
fi
