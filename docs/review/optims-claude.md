I read through the core crates (evaluator, blueprint/MCCFR, search, router, GPU kernels) rather than just the brainstorm doc. You already have `docs/plans/v3-brainstorm.md` — a strong, well-grounded roadmap — so I won't repeat it. Below are engineering ideas it *doesn't* cover, each tied to specific code I read, aimed at squeezing more performance/quality out of the existing architecture without breaking your constitution (no unsafe, no inference-time NN, bit-exact determinism, 16GB fence).

## 1. You already built a faster evaluator path — and aren't using it on CPU

`crates/cham-core/src/eval/mod.rs`: the hot non-flush path (~97% of 7-card evals) does:
```rust
prod = prod.wrapping_mul(PRIMES[rank])   // per card
...
return t.seven_map.get(prod);            // hash-probe LinearMap
```
That's a 64-bit multiply-chain plus a hashed linear-probe lookup. But you *already* built `seven_multiset_ranks: Vec<u32>` (len 50,388), a **direct-indexed** reindexing of the same table by combinadic multiset rank — currently only exposed via `eval_tables()` for the GPU/WGSL backend, which can't do u64 hash keys.

**Idea:** compute the multiset rank incrementally while scanning the 7 cards (accumulate the combinadic sum per rank-count instead of a prime product) and index `seven_multiset_ranks` directly. That removes the hash (multiply+xor+shift) and the probe-chain branch entirely — one guaranteed array load instead of a probabilistic-length linear probe. Given the P1 gate is already ≥100M evals/s, this is exactly the kind of "free" win worth measuring in `benches/eval.rs`: same bit-exact output (it's a pure reindexing of the same map you already prove identical for the GPU), zero abstraction change, zero memory-fence risk (the table's the same size either way).

## 2. Hogwild's real bottleneck is CAS contention on hot nodes, not raw eval speed

`table.rs`'s `Arena::add_f32` is a CAS retry loop. Every preflop-root infoset gets hit on *every single traversal* — under 8-way Hogwild that's the classic degenerate case for CAS: dozens of threads spinning on the same handful of cache lines. Your own comment calls the workload "memory-bound."

**Idea:** give the small set of always-hot nodes (root + first 1-2 decision points, identifiable cheaply by depth) thread-local, non-atomic accumulators that only get reduced into the shared arena at snapshot cadence (you already have a snapshot loop in `trainer.rs`) — essentially a targeted version of what Snapbatch does generically, but specifically for the nodes where CAS contention is worst. This is cheap to try (just route `RegretSink` differently for depth ≤ 1) and should show up directly as reduced wall-clock at fixed thread count in `benches/mccfr.rs`.

Related, smaller: `Slot`/row layout in `Arena` packs rows contiguously with no padding — two adjacent infosets' rows can share a cache line, so Hogwild writes to one infoset can false-share with reads/writes to its neighbor. Worth checking whether padding hot rows to 64B changes anything under contention; if not, drop it (cheap to test, easy kill criterion).

## 3. Snapbatch could be made bit-exact — right now only single-thread is

`ThreadMode::Snapbatch`'s `DeltaBuffer::flush` sorts and merges deltas, which is good, but the *order in which worker buffers reach the arena* across a full iteration is still scheduler-dependent, so multi-threaded runs aren't reproducible — only `Deterministic` (1 thread) is, per your own doc comments. Given plain float adds are associative in the sense that matters here (you're not chasing IEEE bit-identity across *addition order* claims elsewhere), you could get a *stronger* guarantee cheaply: at each snapshot boundary, drain each worker's `DeltaBuffer` in a fixed worker-index order (a short barrier) rather than whenever they happen to flush. That turns "Snapbatch is fast but stochastic" into "Snapbatch is fast and reproducible," which upgrades your correctness fence (bit-exact determinism) to cover the multi-threaded path instead of carving it out. This is a genuinely underused lever — most MCCFR implementations never bother making parallel training reproducible, and you're one barrier away from it.

## 4. DCFR-style regret discounting, not just CFR+ + linear averaging

`averaging_weight()` gives you linear averaging plus a single γ=0.9 discount for Robust mode. The regret update itself is plain CFR+ (floor at 0). Brown & Sandholm's Discounted CFR generalizes this with three independent exponents — α on positive regret, β on negative regret, γ on strategy-sum weight — and empirically converges faster than CFR+ alone in many games, especially early iterations where CFR+'s zero-floor can lock in bad early strategies. Since your negative regrets are already floored at zero (CFR+), the interesting knob you're missing is **α < 1 on early positive-regret accumulation** (discount early positive regret too, not just weight it less in averaging) — worth an A/B against your existing A1 CFR+ plan using the same abstraction-local exploitability metric (C3) you're already building. Cheap to gate, cheap to kill.

## 5. Persist the river-subgame cache across sessions, not just within one process

`cache.rs`'s L1 is `Mutex<HashMap<u64, Arc<Subgame>>>`, process-global, evicted wholesale past 256 entries — and your own numbers show warm hits are **189 µs vs 35 ms cold**, a ~185× difference. But a fresh agent process starts every session with an empty cache. In a real match, effective stack and pot-normalized spots recur constantly (same SPR bands come up over and over at a fixed table). You already have a proven mmap-load pattern at 43 µs/load for blueprint artifacts.

**Idea:** periodically (or at session end) serialize the L1 cache's built subgames to a memory-mapped file, keyed by the same content hash, and load it back at agent startup. This turns a large fraction of session-1 "cold" solves into "warm" solves for free, which directly funds A3's plan to spend more of the 150ms self-cap on deeper iteration counts — cache warming is compute you get to skip entirely rather than compute you have to justify spending.

## 6. EMD-based potential-aware clustering for A2's bucket rebuild

The brainstorm's A2 says "equity distributions, not just mean EHS" without specifying the algorithm. The concrete, well-established technique (Johanson et al., Ganzfried & Sandholm) is: histogram each hand's equity against a fixed set of opponent-range buckets on the *next* street, then cluster with a distance metric that respects distribution shape — Earth Mover's Distance, not Euclidean, on the mean. EMD between two sorted 1-D histograms is cheap (O(bins), it's just the L1 distance between CDFs) and embarrassingly GPU-parallelizable, matching your existing GPU-tables-only pattern. This is strictly more information-preserving than mean EHS at the same table byte budget, and it's the actual technique the "richer features" line in A2 is presumably gesturing at — worth naming explicitly so the implementation doesn't drift toward a weaker heuristic (e.g. adding raw equity-variance as a feature, which is a much weaker signal than EMD clustering).

## 7. Multiple biased leaf continuation strategies for A3's turn solving (DeepStack's real lesson)

When A3 extends real-time solving to turn subgames, a single fixed continuation strategy at the solve's leaves (what happens beyond the solved depth) is itself exploitable — this was the specific failure mode DeepStack's authors solved with **multiple leaf strategies** (e.g., a "call-heavy" and a "fold-heavy" continuation, blended by a small combinator solved *within* the subgame) rather than one blueprint-derived continuation. Your `prior.rs` visit-confidence flattening handles *unvisited* paths gracefully, but a single confident-but-wrong leaf strategy at solve depth is a different bug class. Worth adding 2-3 leaf variants (perturbations of the blueprint prior) to the turn-subgame solver's leaf set as a small, bounded, offline-computable addition — it doesn't touch the online decision path's determinism, just gives the solver more to be robust against.

## 8. Bayesian sequential update instead of exponential hysteresis for the router

`runtime.rs`'s `weights_for_hand` blends via a fixed α (hand-to-hand hysteresis) — an ad hoc exponential smoother. A more sample-efficient and more interpretable alternative: treat each specialist's fit as a Beta-Binomial posterior over the session's observed action-classification features, and combine with the softmax prior by adding log-likelihoods (proper Bayesian fusion) rather than linearly blending probabilities. This gives you a real posterior variance you can use directly as B2's confidence gate threshold (currently presumably a tuned constant) instead of an arbitrary cutoff — and it converges faster on strong early evidence while degrading gracefully on weak evidence, which is exactly the property B2 is reaching for by hand.

## 9. SwissTable-style probing for `RegretTable`

Minor but real: `hash_key` + linear probing with 70%-load doubling is fine at small scale, but at 200bb-depth table sizes (tens of millions of infosets, per your own memory-fence math) naive linear probing has unbounded worst-case probe-chain length near load factor 0.7. `hashbrown`'s SIMD-probed SwissTable design (already effectively what `std::HashMap` uses under the hood) gives predictable short probe sequences and better cache behavior for the exact "memory-bound" workload your comments already flag. This is a self-contained swap in `table.rs`'s slot-finding logic, doesn't change the `RegretSink` API, and is measurable directly against your existing `benches/mccfr.rs`.

---

None of these require leaving the fence, adding a neural net, or touching the online decision path's determinism — they're either free wins on data structures you've already built (items 1, 5), targeted contention fixes (2, 9), an upgrade to a correctness guarantee you're currently missing (3), or named, citable SOTA techniques (4, 6, 7, 8) that sharpen items your own brainstorm already gestures at but doesn't fully specify. Want me to write any of these up as a proper spec doc (matching your `docs/SPECS/` format) or sketch the actual Rust diff for one — I'd suggest starting with #1 (evaluator) and #5 (cache persistence), since both are near-zero-risk and directly measurable against benches you already have?
