#!/usr/bin/env bash
# Run the full-abstraction bp + ladder chain once buckets-full is complete.
# Watches artifacts/buckets-full/turn.bin; fires when present. Logs to
# artifacts/full-agent.log. Does nothing if already complete.
#
# Launch:
#   nohup nice -n 10 bash scripts/run-full-agent.sh \
#     > artifacts/full-agent-driver.log 2>&1 &
set -uo pipefail
cd "$(git rev-parse --show-toplevel)"
LOG=artifacts/full-agent.log
: > "$LOG"
log() { echo "[$(date +%H:%M:%S)] $*" | tee -a "$LOG"; }

log "watching artifacts/buckets-full/ for turn.bin..."

# 1) Wait for buckets
for i in $(seq 1 720); do   # up to 6 h
  if [ -f artifacts/buckets-full/turn.bin ] \
     && [ -f artifacts/buckets-full/meta.json ] \
     && ! pgrep -f "target/release/chameleon train-buckets" >/dev/null; then
    log "buckets ready"
    break
  fi
  sleep 30
done
if [ ! -f artifacts/buckets-full/turn.bin ]; then
  log "FAIL: buckets never appeared after 6 h"
  exit 1
fi

mkdir -p artifacts/blueprints-full

# 2) Robust bp
log "train robust full-abstraction bp (500k iters)..."
cargo run -q --release -p cham-cli -- train-bp \
  --mode robust --iters 500000 --depth 100 --seed 7 \
  --config config/abstraction.toml \
  --buckets artifacts/buckets-full \
  --out artifacts/blueprints-full \
  --threads 4 2>&1 | tee -a "$LOG" | tail -3

# 3) 4 experts
for opp in nit tag lag station; do
  log "train $opp (500k iters)..."
  cargo run -q --release -p cham-cli -- train-bp \
    --mode exploit --opponent "arch:$opp" \
    --iters 500000 --depth 100 --seed 7 \
    --config config/abstraction.toml \
    --buckets artifacts/buckets-full \
    --out "artifacts/blueprints-full/$opp" \
    --threads 4 2>&1 | tee -a "$LOG" | tail -2
done

# 4) Assemble artifacts/agent-full (SEPARATE from tiny)
log "assemble artifacts/agent-full"
rm -rf artifacts/agent-full
mkdir -p artifacts/agent-full/experts/0 artifacts/agent-full/experts/1 \
         artifacts/agent-full/experts/2 artifacts/agent-full/experts/3 \
         artifacts/agent-full/robust
cp -a artifacts/buckets-full artifacts/agent-full/buckets
cp -a config/abstraction.toml artifacts/agent-full/abstraction.toml
cp -a artifacts/blueprints-full/robust-7/policy/policy.bin artifacts/agent-full/robust/policy.bin
i=0
for opp in nit tag lag station; do
  cp -a "artifacts/blueprints-full/$opp/exploit-7/policy/policy.bin" \
        "artifacts/agent-full/experts/$i/policy.bin"
  i=$((i+1))
done

# 5) Ladder: swap in full agent, run, swap back
log "ladder full-abstraction agent (keeping tiny as agent-tiny)"
mv artifacts/agent artifacts/agent-tiny-backup
cp -a artifacts/agent-full artifacts/agent
cargo run -q --release -p cham-cli -- ladder --fast --agent full 2>&1 | tee -a "$LOG"
mv artifacts/agent-tiny-backup artifacts/agent

log "DONE — full-abstraction ladder above"
