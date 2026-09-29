#!/usr/bin/env bash
# delay0 + eps=0.02 combined experiment (2026-09-29).
#
# The two best single levers from the evening are:
#   * CHAM_AVG_DELAY=0  -> BB 14706 -> 13618 (-7.4%)
#   * CHAM_TRAIN_EPS=0.02 -> BB 14706 -> 13652 (-7.2%)
# They act on different stages (average weight vs current iterate), so
# they might compose. This queues the combination after the current
# pipelines finish.
set -uo pipefail
cd "$(git rev-parse --show-toplevel)"

log() { echo "[$(date "+%H:%M:%S")] $*"; }

log "waiting for dcfr-alpha and freeze-diag pipelines..."
while pgrep -f "dcfr-alpha-experiments" >/dev/null; do sleep 60; done
while pgrep -f "freeze-diag-2026-09-29" >/dev/null; do sleep 60; done
log "dependencies done"

out="artifacts/par-20M-delay0-eps02"
if [ -f "$out/robust-7/policy/policy.bin" ]; then
  log "skip training, artifact already present"
else
  log "training 20M tiny robust with CHAM_AVG_DELAY=0 CHAM_TRAIN_EPS=0.02"
  mkdir -p "$out"
  CHAM_AVG_DELAY=0 CHAM_TRAIN_EPS=0.02 target/release/chameleon train-bp \
    --mode robust --iters 20000000 --depth 100 --seed 7 \
    --config config/abstraction-tiny.toml \
    --buckets artifacts/buckets-tiny \
    --out "$out" \
    --threads 4 --thread-mode hogwild \
    > "artifacts/par-20M-delay0-eps02.log" 2>&1
fi

log "LBR (1000 deals)"
CHAM_EXPLOIT_BP="$PWD/$out/robust-7/policy" \
CHAM_EXPLOIT_BUCKETS="$PWD/artifacts/buckets-tiny" \
CHAM_EXPLOIT_CONFIG="$PWD/config/abstraction-tiny.toml" \
CHAM_EXPLOIT_DEALS=1000 \
  cargo bench -q -p cham-blueprint --bench exploitability \
  > "artifacts/par-20M-delay0-eps02-lbr.criterion.log" \
  2> "artifacts/par-20M-delay0-eps02-lbr.stderr.log"
grep "exploitability\[bp" "artifacts/par-20M-delay0-eps02-lbr.stderr.log" \
  > "artifacts/par-20M-delay0-eps02-lbr.log"

log "rm_freeze"
/tmp/rm_freeze/target/release/rm_freeze "$out/robust-7/table.snap" \
  > "artifacts/par-20M-delay0-eps02-freeze.txt" 2>&1

log "DONE"
