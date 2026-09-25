# Performance backlog

Items worth a proper session each, not drive-by work. Reviewed against
concrete code. All respect the constitution: no unsafe, no inference-time
NN, bit-exact determinism, 16 GB fence.

## Ready to try, low risk

### B-1. Multiset-rank eval path (CPU evaluate7)
`cham-core/src/eval/mod.rs` already builds `seven_multiset_ranks: Vec<u32>`
(50,388 u32s) -- same table keyed by combinadic rank, not prime product --
but only exposes it via `eval_tables()` for the GPU/WGSL backend.

Hypothesis: replace the hot non-flush path's `prime_product + LinearMap.get`
with a direct index into `seven_multiset_ranks`.

Reality check: "incremental multiset rank while scanning" is mathematically
wrong -- combinadic rank needs sorted counts. The variant is:
    counts[13] = 0
    for each card: counts[rank] += 1
    rank = multiset_rank(&counts)   // 13-iteration loop
    dense = seven_multiset_ranks[rank]
vs current: 7 multiplies + splitmix hash + linear probe. Must be measured.

Baseline recorded: eval_evaluate7 mean ~31.86 us / 1000 evals.
Effort: 30-60 min. Risk: low (bit-exact by construction).

### B-2. River-subgame cache persistence
`cham-search/src/cache.rs` L1 is process-global, wholesale-evicted past
256 entries. Warm 189 us vs 35 ms cold (185x). Serialize to an mmap file
keyed by content hash; load at startup. Reduces session-1 cold cost.
Effort: 3-5 h. Risk: medium (format-version discipline).

### B-3. Targeted Hogwild hot-node accumulation
`table.rs::Arena::add_f32` CAS retry; preflop-root infosets hit every
traversal. Depth <= 1 nodes -> thread-local accumulators, reduced at
snapshot cadence. Effort: needs profiling first. Risk: medium (training
math; needs Snapbatch-parity test).

## Named SOTA techniques worth writing up as specs

### B-4. Snapbatch bit-exact multi-thread ordering
Drain each worker's DeltaBuffer in fixed worker-index order at each snapshot
boundary (one barrier). Upgrades "fast but stochastic" to "fast and
reproducible." Cheap, high-value for the correctness fence.

### B-5. DCFR-style regret discounting
alpha < 1 discount on positive regret accumulation. A/B against CFR+ on
the abstraction-local exploitability metric (C3). Research item.

### B-6. EMD-based potential-aware clustering
v3 A2 bucket rebuild should use Earth Mover's Distance between per-hand
equity histograms (Johanson et al., Ganzfried & Sandholm), not mean EHS.
Multi-day (rebuilds bucketing).

### B-7. Multi-leaf continuation strategies for turn solving
DeepStack's lesson: single fixed leaf continuation is itself exploitable.
2-3 perturbed-blueprint leaf variants blended by a small combinator. v3.

### B-8. Bayesian sequential router update
`runtime.rs` weights_for_hand uses fixed-alpha exponential smoothing.
Replace with Beta-Binomial posterior + log-likelihood fusion. Research.

### B-9. SwissTable-style RegretTable probing
`hashbrown` SIMD-probed SwissTable for RegretTable slot finder. Medium
risk; needs a profile to justify.

## When to revisit

After tonight's tiny-agent experiment lands real ladder numbers, we'll know
where bottlenecks actually are. B-1 through B-3 are candidates only if a
bench shows help; B-4 is worth doing regardless; B-5 through B-9 are v3.
