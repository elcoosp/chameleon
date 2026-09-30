#!/usr/bin/env bash
# Ladder measurement for the tiny-5M SOTA robust policy (2026-09-30).
#
# Builds two throwaway bundles from agent-honest:
#   * artifacts/agent-robust-honest   (agent-honest's robust policy)
#   * artifacts/agent-robust-par5m    (par-5M's robust policy)
# Then runs `ladder --fast --agent robust-only` on both and diffs.
#
# Rationale: the 09-28 ladder numbers were for `--agent full` (argmax
# routing across 4 experts). The current SOTA is a single robust policy
# trained to 5M iters, and its ladder was never measured.
set -uo pipefail
cd "$(git rev-parse --show-toplevel)"

log() { echo "[$(date "+%H:%M:%S")] $*"; }

build_bundle() {
  local dst="$1"
  local robust_src="$2"
  rm -rf "$dst"
  mkdir -p "$dst"
  cp -R artifacts/agent-honest/abstraction.toml "$dst/"
  cp -R artifacts/agent-honest/buckets "$dst/"
  cp -R artifacts/agent-honest/experts "$dst/"
  if [ -f artifacts/agent-honest/router.bin ]; then
    cp artifacts/agent-honest/router.bin "$dst/"
  fi
  mkdir -p "$dst/robust"
  cp "$robust_src/policy.bin" "$dst/robust/policy.bin"
  if [ -f "$robust_src/provenance.json" ]; then
    cp "$robust_src/provenance.json" "$dst/robust/provenance.json"
  fi
}

log "building bundle: artifacts/agent-robust-honest"
build_bundle artifacts/agent-robust-honest \
  artifacts/agent-honest/robust

log "building bundle: artifacts/agent-robust-par5m"
build_bundle artifacts/agent-robust-par5m \
  artifacts/par-5M/robust-7/policy

log "ladder --fast --agent robust-only @ agent-robust-honest"
CHAM_AGENT_BUNDLE="$PWD/artifacts/agent-robust-honest" \
  target/release/chameleon ladder --fast --agent robust-only \
  > artifacts/ladder-robust-honest.log 2>&1 || \
  echo "  ladder for honest failed (see artifacts/ladder-robust-honest.log)"

log "ladder --fast --agent robust-only @ agent-robust-par5m"
CHAM_AGENT_BUNDLE="$PWD/artifacts/agent-robust-par5m" \
  target/release/chameleon ladder --fast --agent robust-only \
  > artifacts/ladder-robust-par5m.log 2>&1 || \
  echo "  ladder for par5m failed (see artifacts/ladder-robust-par5m.log)"

log "=== results: agent-robust-honest ==="
cat artifacts/ladder-robust-honest.log

log "=== results: agent-robust-par5m ==="
cat artifacts/ladder-robust-par5m.log

log "DONE"
