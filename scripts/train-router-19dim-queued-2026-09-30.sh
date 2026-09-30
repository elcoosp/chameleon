#!/usr/bin/env bash
# Train and gate-check the 19-dim honest router (2026-09-30).
# Waits for the raw-opponent-19 collect to finish.
set -uo pipefail
cd "$(git rev-parse --show-toplevel)"

LOCKDIR="${TMPDIR:-/tmp}/chameleon-train-19dim.lock"
if ! mkdir "$LOCKDIR" 2>/dev/null; then
  echo "another 19-dim train is already running"; exit 0
fi
trap 'rm -rf "$LOCKDIR"' EXIT INT TERM

log() { echo "[$(date "+%H:%M:%S")] $*"; }

log "waiting for raw-opponent-19 collect to finish..."
while pgrep -f "collect.*raw-opponent-19" >/dev/null; do sleep 30; done
log "collect done"

if [ ! -f artifacts/router_raw_19.rbin ]; then
  log "ERROR: artifacts/router_raw_19.rbin missing"; exit 1
fi

# Verify .rbin header
python3 - << 'PYEOF'
import struct, os
p = "artifacts/router_raw_19.rbin"
with open(p, "rb") as f:
    magic = f.read(4)
    ver, rows, feats = struct.unpack("<III", f.read(12))
print(f"  {p}: magic={magic!r} v{ver} rows={rows} feats={feats}")
expected = 16 + rows * (4*feats + 4)
actual = os.path.getsize(p)
if expected != actual:
    print(f"  SIZE MISMATCH: expected {expected} actual {actual}")
    raise SystemExit(1)
PYEOF

log "=== train-router on 19-dim ==="
target/release/chameleon train-router \
  --rows artifacts/router_raw_19.rbin \
  --out artifacts/routers/v19 \
  > artifacts/train-router-raw-19.log 2>&1
log "  done; log: artifacts/train-router-raw-19.log"
cat artifacts/train-router-raw-19.log

log "DONE"
