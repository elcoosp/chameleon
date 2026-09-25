# cham-engine

The abstraction layer (SPECS/02). Infoset keys are PURE functions of
(hole, board, geometry) — zero Monte Carlo on the encode path.

- `canon.rs` — Waugh-style 24-permutation suit-isomorphism canonical keys.
- `tables.rs`/`build.rs` — offline flop/turn bucket tables (CDF features →
  k-means) + committed tiny-abstraction artifacts (`artifacts/buckets-tiny`).
- `buckets.rs` — flop/turn table lookup; river = exact equity quantile ×
  8-class board texture (board-aware by construction).
- `ladder.rs` — canonical slot ladders (≤ 2 sizes + jam), raise cap,
  pseudo-harmonic off-tree weights.
- `encoder.rs` — key = street|pos|SPR band|bucket|legal mask|action-seq window
  (FNV-mixed); invariant I8: row width == popcount(mask).
