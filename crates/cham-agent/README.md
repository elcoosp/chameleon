# cham-agent

Composition (SPECS/07): tracker + router + experts + searcher → one `Agent`,
in config-driven modes (two modes differ only in knobs, never code paths).

- `tracker.rs` — EWM stats + opportunity counts from `&PublicHistory` only
  (type-level leak discipline); maturity shrink; EV-trend z-score.
- `pipeline.rs` — per-hand FROZEN weights; per-decision reach-weighted mixture
  `σ_mix ∝ Σ w_k·π_k·σ_k` (π_k = own-line reach under expert k); uncovered
  expert → robust substitution; uncovered robust → uniform legal + record.
- `loader.rs` — blake3-checked, depth-flexible artifact loading (mixed-depth
  bundles are hard errors).
- `trace.rs` — decision records via cham-rec; traces are the only persistence
  of weights.
