#!/usr/bin/env bash
# Run the medium-abstraction experiment after buckets are built and the
# current training job is done. Idempotent: skips anything already done.
set -uo pipefail
cd "$(git rev-parse --show-toplevel)"

if [ ! -f artifacts/buckets-medium/meta.json ]; then
  echo "medium buckets not built yet — run:"
  echo "  target/release/chameleon train-buckets --config config/abstraction-medium.toml --out artifacts/buckets-medium"
  exit 1
fi

for iters in 5000000 20000000; do
  out="artifacts/par-medium-${iters}"
  if [ -f "$out/robust-7/policy/policy.bin" ]; then
    echo "skip: $out already done"
    continue
  fi
  mkdir -p "$out"
  echo "=== medium, $iters iters, parallel 4 ==="
  target/release/chameleon train-bp \
    --mode robust --iters "$iters" --depth 100 --seed 7 \
    --config config/abstraction-medium.toml \
    --buckets artifacts/buckets-medium \
    --out "$out" \
    --threads 4 --thread-mode hogwild

  echo "=== LBR ==="
  CHAM_EXPLOIT_BP="$PWD/$out/robust-7/policy" \
  CHAM_EXPLOIT_BUCKETS="$PWD/artifacts/buckets-medium" \
  CHAM_EXPLOIT_CONFIG="$PWD/config/abstraction-medium.toml" \
  CHAM_EXPLOIT_DEALS=200 \
    cargo bench -q -p cham-blueprint --bench exploitability 2>&1 \
    | grep -E "exploitability\[bp"
done
echo "=== DONE ==="
