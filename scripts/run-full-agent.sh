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
# H-14 fix (2026-09-27): the old sequence
#     mv artifacts/agent artifacts/agent-tiny-backup
#     cp -a artifacts/agent-full artifacts/agent
#     <run ladder>
#     mv artifacts/agent-tiny-backup artifacts/agent
# was broken: after the cp recreates `artifacts/agent` as a directory, the
# final mv has an EXISTING dst dir, so POSIX mv semantics NEST the source
# INSIDE it (`artifacts/agent/agent-tiny-backup/`) instead of restoring the
# tiny agent at the top level. The full agent silently stayed installed and
# a second run compounded the nesting. Also the ladder's exit code was
# never checked.
#
# Fix: rm the destination before restoring, and gate on the ladder rc so a
# failing ladder does not silently look like success.
if [ ! -d artifacts/agent ]; then
  log "FAIL: artifacts/agent does not exist to back up"
  exit 1
fi
# Preserve any pre-existing backup rather than clobbering it silently
if [ -e artifacts/agent-tiny-backup ]; then
  log "FAIL: artifacts/agent-tiny-backup already exists — remove it first"
  exit 1
fi
mv artifacts/agent artifacts/agent-tiny-backup
# Make sure the restore happens even if the ladder fails or the shell
# dies: this is the whole point of the backup.
restore_tiny() {
  rm -rf artifacts/agent
  mv artifacts/agent-tiny-backup artifacts/agent
}
trap 'restore_tiny' EXIT INT TERM
cp -a artifacts/agent-full artifacts/agent
ladder_rc=0
cargo run -q --release -p cham-cli -- ladder --fast --agent full 2>&1 | tee -a "$LOG" || ladder_rc=$?
# Reinstall the tiny agent explicitly before reporting the ladder's status.
restore_tiny
trap - EXIT INT TERM
if [ "$ladder_rc" -ne 0 ]; then
  log "FAIL: ladder exited $ladder_rc — tiny agent restored"
  exit "$ladder_rc"
fi

log "DONE — full-abstraction ladder above"
