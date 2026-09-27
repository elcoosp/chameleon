#!/usr/bin/env bash
# EXP-015 60-cell router-manipulation grid (v6 runbook item 8).
# Flags adapted to the shipped CLI (--snapshot/--buckets/--config; the
# runbook's positional form predates the current arg surface).
set -uo pipefail
cd "$(git rev-parse --show-toplevel)"
mkdir -p artifacts/exp-015-grid
for switch in 10 20 40 80 150; do
  for n0 in 4 8 16 32; do
    for temp in 0.5 0.7 1.0; do
      out="artifacts/exp-015-grid/switch${switch}-n0${n0}-temp${temp}.json"
      # L-23 fix (2026-09-27): this 60-cell grid was running in DEBUG mode
      # (`cargo run -q`) while every sibling experiment script uses
      # `--release`. Debug is 10-50× slower, risking truncated runs.
      cargo run -q --release -p cham-cli -- self-exploit \
        --snapshot artifacts/blueprints-tiny/robust-7 \
        --buckets artifacts/buckets-tiny \
        --config config/abstraction-tiny.toml \
        --deals 2000 --switch-at "$switch" --router-n0 "$n0" --router-temp "$temp" \
        > "$out" 2>&1
      echo "done: switch=$switch n0=$n0 temp=$temp -> $out"
    done
  done
done
