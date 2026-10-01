#!/usr/bin/env bash
# Retrain the 4 tiny experts at 5M iters each (2026-09-30).
#
# Motivation: the argmax ladder is dominated by the 4 experts. They were
# trained at 500k iters (retrain-tiny-honest.sh). The 5M robust upgrade
# was worth +100 on the robust-only ladder; the analogous expert upgrade
# is unmeasured. This script retrains the experts at 5M using the
# parallel trainer (2.4x speedup over serial). Same seeds, same config,
# same 500k→5M×10 iteration ratio.
#
# Output: artifacts/blueprints-tiny-honest-5M/experts/{nit,tag,lag,station}
# Assembles into: artifacts/agent-honest-5M-experts/
set -uo pipefail
cd "$(git rev-parse --show-toplevel)"

LOCKDIR="${TMPDIR:-/tmp}/chameleon-5M-experts.lock"
if ! mkdir "$LOCKDIR" 2>/dev/null; then
  echo "another 5M-expert retrain is already running (lock=$LOCKDIR); exiting"
  exit 0
fi
trap 'rm -rf "$LOCKDIR"' EXIT INT TERM

log() { echo "[$(date "+%H:%M:%S")] $*"; }

CFG=config/abstraction-tiny.toml
BUCKETS=artifacts/buckets-tiny
OUT=artifacts/blueprints-tiny-honest-5M
AGENT=artifacts/agent-honest-5M-experts
ITERS=5000000
SEED=7
THREADS=4

mkdir -p "$OUT"

for opp in nit tag lag station; do
  sub="$OUT/$opp"
  if [ -f "$sub/exploit-$SEED/policy/policy.bin" ]; then
    log "$opp: already trained, skipping"
    continue
  fi
  log "=== training expert: $opp (5M iters) ==="
  if ! target/release/chameleon train-bp \
    --mode exploit --opponent "arch:$opp" \
    --iters "$ITERS" --depth 100 --seed "$SEED" \
    --config "$CFG" --buckets "$BUCKETS" \
    --out "$sub" \
    --threads "$THREADS" --thread-mode hogwild \
    > "artifacts/retrain-5M-expert-$opp.log" 2>&1
  then
    log "  ERROR: $opp training failed (exit != 0); see log"
    exit 1
  fi
  # Guard: refuse to proceed if the produced policy is a stub
  # (the 2026-09-30 partial-failure mode produced 308-byte stubs on
  # empty-table parallel exploit runs — see RETRAIN-5M-EXPERTS-PARTIAL).
  produced="$sub/exploit-$SEED/policy/policy.bin"
  sz=$(wc -c < "$produced" 2>/dev/null || echo 0)
  if [ "$sz" -lt 100000 ]; then
    log "  ERROR: $opp policy is only $sz bytes (expected > 100 KB); aborting"
    exit 1
  fi
  log "  $opp done ($sz bytes)"
done

log "=== assemble bundle: $AGENT ==="
rm -rf "$AGENT"
mkdir -p "$AGENT/experts/0" "$AGENT/experts/1" "$AGENT/experts/2" "$AGENT/experts/3" "$AGENT/robust"
cp -a "$BUCKETS" "$AGENT/buckets"
cp -a "$CFG" "$AGENT/abstraction.toml"
# robust stays at 500k for this experiment (agent-honest's robust)
cp -a artifacts/agent-honest/robust/policy.bin "$AGENT/robust/policy.bin"
# router stays the synthetic one from agent-honest
if [ -f artifacts/agent-honest/router.bin ]; then
  cp artifacts/agent-honest/router.bin "$AGENT/router.bin"
fi
i=0
for opp in nit tag lag station; do
  cp -a "$OUT/$opp/exploit-$SEED/policy/policy.bin" "$AGENT/experts/$i/policy.bin"
  i=$((i+1))
done
log "  assembled: $AGENT"

log "=== ladder --fast --agent full on $AGENT ==="
CHAM_AGENT_BUNDLE="$PWD/$AGENT" \
  target/release/chameleon ladder --fast --agent full \
  > artifacts/ladder-5M-experts-full.log 2>&1
log "  written artifacts/ladder-5M-experts-full.log"

log "=== ladder --fast --agent full-mixture on $AGENT ==="
CHAM_AGENT_BUNDLE="$PWD/$AGENT" \
  target/release/chameleon ladder --fast --agent full-mixture \
  > artifacts/ladder-5M-experts-mixture.log 2>&1
log "  written artifacts/ladder-5M-experts-mixture.log"

log "DONE"
