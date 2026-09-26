# CHAMELEON v3 — Execution Roadmap (vetted, code-grounded)

> **Status:** proposal, ready to convert into `EXP-*` / `G3.x` gates.
> **Inputs reviewed:** `docs/plans/v2-roadmap.md`, `docs/plans/v3-brainstorm.md`
> (draft, several items tagged `[VERIFY]`), `docs/review/optims-claude.md`,
> `docs/backlog/perf.md`, `docs/reports/competitiveness.md`,
> `worklog.md`, and the actual source in `crates/cham-{core,blueprint,search,eval,cli,router,gpu}`.
> **What this document is:** the union of v2/v3/optims-claude, re-triaged
> against what the code *actually does today*, reordered by EV-per-hour, and
> turned into diffs or precise implementation plans where the source
> justified one. Anything that turned out to already be shipped is marked
> **[ALREADY DONE]** and removed from scope. Anything I could not verify
> against code I could see is marked **[VERIFY]** — treat those as hypotheses,
> not commitments.

---

## 0. The one thing that governs this whole plan

`docs/plans/v3-brainstorm.md` already reaches the correct conclusion and I'm
not relitigating it: **you cannot tune what you cannot measure.** The ladder
numbers in the repo today (`-59.5 bb/100` vs `arch:lag`, `±670.8 mb/seating`
vs `pnash:overfold:0.15`) are **fallback-contaminated and variance-blind**,
per `cmd/guard.rs` and the ladder's own error bars. Every item below is
sequenced so that:

1. **Nothing ships as a default without a ledger entry beating the v1/v2
   baseline**, exactly per the existing `EXP-*` + Holm + ledger discipline —
   this roadmap adds zero new trust mechanisms.
2. **Measurement infrastructure comes before optimization.** The fastest way
   to waste the next month is to tune a blueprint against a ±67 bb/100 noise
   floor. Track C (below) is first, not last, and it's also the cheapest set
   of changes in this document.
3. **Every item states its file(s), its kill criterion, and its cost class**
   (drop-in / half-day / multi-day) so it can become an `experiments/EXP-0NN.toml`
   directly.

Cost classes used below: **[S]** small (<1 day, mostly mechanical),
**[M]** medium (1–3 days), **[L]** large (multi-day / a training weekend).

---

## 1. Immediate: fix and parallelize the A/B/ladder path (quickness of A/B testing)

This is the highest-leverage, lowest-risk item in the whole document, and it
directly answers "quickness of A/B testing" — it's a **bug fix that is also
a 4–8× wall-clock win**, found by diffing two code paths that should behave
identically but don't.

### 1.1 `cham-eval/src/ab.rs::AbRunner::run_shared` is serial *and* leaks tracker state across opponents — `ladder.rs` already solved this correctly [S]

Compare the two real-hero evaluation paths in the repo:

- **`cham-cli/src/cmd/ladder.rs::run_opponent`** (correct, and already fast):
  builds a **fresh** `CountingHero` per opponent (comment: *"one CountingHero
  per opponent match (agents own per-hand tracker state, never shared across
  matches)"*), runs opponents on **one thread per opponent** via
  `std::thread::scope`, in **250-deal SPRT-chunked ranges** via
  `MatchRunner::run_shared_range`, with deterministic seed derivation
  (`base_seed ^ (index << 32)`) so parallel output is bit-identical to a
  sequential run.
- **`cham-eval/src/ab.rs::AbRunner::run_shared`** — the path `cmd/ab.rs`
  calls whenever *either* arm needs trained artifacts, i.e. **every A/B test
  that actually matters** — does the opposite on both counts: it reuses
  **one mutable `hero_a` / `hero_b` across the entire opponent pool**, in a
  **plain sequential `for` loop**, calling `MatchRunner::run_shared` (no
  range chunking, no threads).

Two consequences, one correctness, one performance:

- **Correctness risk:** the router's belief state (tracker EWM stats,
  archetype posterior) carries over from `arch:nit` into `arch:tag` into
  `arch:lag` within one `ab` invocation, because it's the same live
  `ChameleonAgent` instance walking the whole pool. `ladder.rs`'s own doc
  comment says this is deliberately *not* how a shared hero should be used
  across distinct opponent identities. This can bias exactly the numbers
  `EXP-001` (`G2-primary`, the mixture-vs-robust thesis verdict) depends on.
- **Performance:** `ab` is the command the whole `EXP-*` registry runs
  through (`experiments/EXP-001..023*.toml`), and it's the one path that
  *doesn't* use the parallelism pattern the codebase already proved out in
  `ladder.rs`. On an 8-core M1 this is roughly an **8× wall-clock** loss for
  every pool-sized A/B run, and it compounds every screening iteration in
  the v3 sequencing plan (§6 below runs *many* A1/A4/B1/B2 A/B rounds).

**Fix: port `ladder.rs`'s pattern into `AbRunner::run_shared`.**

```rust
// crates/cham-eval/src/ab.rs

impl AbRunner {
    /// Parallel, per-opponent-isolated shared-hero A/B (replaces the old
    /// sequential `run_shared` body). Each opponent gets its OWN hero
    /// instances (session isolation, matching `ladder.rs`'s `run_opponent`
    /// contract) and its own thread; seeds are derived exactly as in
    /// `ladder.rs` / the old `run` (factory) path, so results are
    /// deterministic regardless of thread scheduling.
    pub fn run_shared<FA, FB>(
        spec: &AbSpec,
        pool: &[cham_opponents::OpponentSpec],
        hero_factory_a: &FA,
        hero_factory_b: &FB,
        depth_bb: i64,
        rec: Option<&mut Recorder>,
    ) -> Result<AbVerdict, EvalError>
    where
        FA: Fn() -> Result<Box<dyn Agent>, String> + Sync,
        FB: Fn() -> Result<Box<dyn Agent>, String> + Sync,
    {
        let per_opp_results: Vec<Result<(PerOppDelta, Vec<f64>), EvalError>> =
            std::thread::scope(|s| {
                let handles: Vec<_> = pool
                    .iter()
                    .enumerate()
                    .map(|(i, opp)| {
                        s.spawn(move || -> Result<(PerOppDelta, Vec<f64>), EvalError> {
                            // Fresh, session-isolated hero per opponent —
                            // same contract as ladder.rs::run_opponent.
                            let mut hero_a = hero_factory_a()
                                .map_err(|e| EvalError::Match(format!("arm a: {e}")))?;
                            let mut hero_b = hero_factory_b()
                                .map_err(|e| EvalError::Match(format!("arm b: {e}")))?;
                            let mk = |arm_seed: u64| MatchSpec {
                                opponent: OpponentSpecDto(opp.id()),
                                deals: spec.deals_per_opp,
                                depth_bb,
                                base_seed: arm_seed ^ ((i as u64) << 32),
                                label: format!("ab:{}/{}/{}", spec.a, spec.b, opp.id()),
                            };
                            let ra = MatchRunner::run_shared(
                                &mk(spec.seeds[0]), hero_a.as_mut(), None,
                            )?;
                            let rb = MatchRunner::run_shared(
                                &mk(spec.seeds[0]), hero_b.as_mut(), None,
                            )?;
                            let da = ra.per_deal_profits.clone().unwrap_or_default();
                            let db = rb.per_deal_profits.clone().unwrap_or_default();
                            let diffs: Vec<f64> =
                                da.iter().zip(db.iter()).map(|(x, y)| x - y).collect();
                            let rng = &mut cham_core::rng::rng_from_seed(spec.seeds[0] ^ 0xAB);
                            let ci = paired_ci(&diffs, spec.conf, rng);
                            let delta = crate::stats::mean(&diffs);
                            Ok((PerOppDelta { opponent: opp.id(), delta_mb: delta, ci }, diffs))
                        })
                    })
                    .collect();
                handles.into_iter().map(|h| h.join().expect("ab thread")).collect()
            });
        // ... reassemble in pool order, SPRT, ledger — identical to existing `run`
    }
}
```

`cmd/ab.rs` changes from passing `&mut hero_a` / `&mut hero_b` to passing
`build_hero`-wrapping closures (mirroring `ladder.rs::run_opponent`'s
`build_chameleon` call). Since blueprint artifacts are mmap-loaded
(`artifact_load`: **43.03 µs/load**, per the competitiveness report), N
independent hero instances cost microseconds, not seconds — there is no real
tradeoff here, only upside. Also add **250-deal SPRT chunking** via
`MatchRunner::run_shared_range` (already exists — `ladder.rs` uses it), so
`ab` gets the same early-stop speedup `ladder` already has.

*Kill criterion:* none needed — this is a correctness-neutral-or-better,
strictly-faster refactor reusing code paths already proven in `ladder.rs`.
Gate: `cham-eval` test suite green + a regression check that the new
parallel `run_shared` produces the same `delta_mb` (within float-sum
reordering tolerance) as the old sequential path on a fixed small pool.

### 1.2 Persist the river-subgame cache across *all* eval commands, not just save/restore once [S]

`cmd/cache_guard.rs`'s `CachePersist` already hydrates/saves
`artifacts/river-cache.bin` for `play`, `ladder`, and `ab` — good, this part
is **[ALREADY DONE]** (worklog: B-2 done, B-10 stage 1 done, postcard
migration done). Two follow-ups that make it actually pay off for A/B
iteration speed rather than just live play:

- **Raise `CACHE_CAP` (currently 256, wholesale-evicted) and switch to an
  LRU/CLOCK eviction instead of clear-everything.** At 256 entries and full
  clears, a single `ab` run touching more distinct SPR bands than that
  throws away *all* of session's warm entries, not just the oldest one. This
  is a `cache.rs` change only (swap `HashMap` clear for an LRU bump list, or
  use the `lru` pattern with an intrusive doubly-linked eviction order); the
  cache stays process-global, content-keyed, and bit-exact by construction
  (§ existing module comment).
- **Warm the cache from a canonical fixture before every `EXP-*` sweep.**
  Since real matches recur at similar SPR bands across arms (that's the
  whole justification for the cache), pre-seeding `river-cache.bin` from a
  short calibration run before a multi-arm sweep (A1/A4/B1/B2 in §6) turns
  the *first* arm's cold solves into the same 189 µs warm path the *last*
  arm gets. Mechanically: one `chameleon warm-cache --pool config/pool.toml`
  subcommand that runs a small fixed session and saves, run once before an
  `EXP-*` sweep's arms.

*Kill criterion:* cache hit rate (`cache_stats()`) measured before/after on
a representative `ab` sweep; if hit rate doesn't move, the LRU change isn't
worth the complexity — keep wholesale eviction.

---

## 2. Track C — make the gates decidable (do this before touching the blueprint)

This is `v3-brainstorm.md`'s own Track C, and its own §6 sequencing already
puts it first for the right reason: the `±670.8 mb/seating` error bar on
`pnash:overfold:0.15` is **26× wider** than the `±25 mb/seating` gate target
— closing that by raw seatings alone is `(670/25)² ≈ 718×` more samples.
I'm not re-deriving this; I'm scoping the implementation.

### 2.1 AIVAT / duplicate-deal variance reduction as the default ledger mode [M]

`cham-eval/src/vr.rs` **already has** `allin_ev_adjusted` — the B4
duplicate-pairing stage wired into `matcheng.rs::allin_adjusted_net`
(flop/turn all-in showdowns replaced by exact equity vs the actual villain
hand). This is real, working variance reduction, already in the hot path of
every match. What's missing, per the brainstorm's own honest accounting:

- **The full AIVAT enumeration stage** (baseline-value subtraction at every
  decision point, not just all-in showdowns) is spec'd (SPECS/08 §5) but
  **does not exist** — confirmed by `worklog.md`'s G2.0 audit: *"`cham-eval/src/vr.rs`
  has only `allin_ev_adjusted`... The full AIVAT enumeration stage does not
  exist yet."*
- **Preflop all-ins are explicitly out of scope today** (`matcheng.rs`
  comment: *"preflop all-ins need ~1.7M completion evals... and keep the
  realized net"*) — this is the single largest remaining variance source in
  the current adjustment, because preflop all-ins get zero luck-removal
  while flop/turn all-ins get full removal.

**Plan:**
1. **[S]** Instrument `vr_stats` (already computes `vr_factor` = ratio of raw
   to adjusted variance) into every `ab`/`ladder` printout as a first-class
   number, not just an internal ratio — so every gate run visibly reports
   "how much did variance reduction already buy us" before deciding whether
   full AIVAT is worth building.
2. **[L]** Full AIVAT baseline-value stage: at each decision point, subtract
   an estimated baseline value (computable offline from the blueprint's own
   average strategy — no new opponent model needed, matching the "use
   tooling you already have" principle from v2's self-exploit idea). This is
   the Track C1 item from the brainstorm; scope it as its own `SPECS/08`
   amendment before writing code, because it changes the ledger's `mb/seating`
   semantics and every downstream gate threshold needs re-deriving against
   the new estimator's variance, not just its mean.
3. **[S]** Preflop all-in completion evals: 1.7M evals sounds large in the
   abstract, but at the CPU rate already measured (**31.6M evals/s**) that's
   **~54 ms** of pure evaluator time per distinct preflop all-in spot — and
   at the GPU builder rate (**3.29e9 evals/s** steady-state) it's **~0.5 µs**.
   This is squarely a `cham-gpu` job per Track D1's job-ranking: an
   offline, bit-exact-validated table (same G1.2 precedent as the turn/flop
   EHS tables), not a live computation. Concretely: enumerate the 1,712,304
   `(hero, villain)` preflop combos once, store exact equities in a table
   the same size class as the existing turn table budget, and look them up
   at `vr_stats` time instead of computing them per-match.

*Kill criterion:* if `vr_factor` from step 1 shows flop/turn all-in
adjustment already captures >90% of achievable variance reduction on the
actual pool mix (i.e., preflop all-ins are rare at the trained agent's
range), steps 2–3 move down the priority stack — measure before building.

### 2.2 Pre-registration + ledger linter [S]

Per brainstorm C2: a small TOML declaring opponent/artifact-hash/seating-count/
pass-condition, checked by a `cmd/lint_ledger.rs` in the same spirit as
`cmd/guard.rs`'s exit-2 discipline. This is genuinely an afternoon of work
(the `LedgerEntry` schema in `cham-eval/src/ledger.rs` already carries most
of the needed fields) and it's the cheapest insurance in this whole document
against "gate shopping."

**[VERIFY]** whether `LedgerEntry` currently carries an artifact-content-hash
field — the brainstorm flags this as unverified (Constitution §6). If it
doesn't, add it now, before any G3.x gate run is committed, because a ladder
number without a bound artifact identity is retroactively unauditable.

### 2.3 Exploitability telemetry as a Criterion-tracked benchmark [M]

Promote the existing P-2 best-response machinery (already proven correct by
the M-1 proof) into `benches/exploitability.rs`: sampled abstraction-local
subgames, reported per street per depth, run on every PR via the existing
`bench.yml` criterion-baseline infrastructure. This gives Track A a
minutes-scale feedback loop instead of a days-scale ladder — every kill
criterion in §3 below reads off this benchmark first, exactly as
`v3-brainstorm.md §3` Track A entries specify.

---

## 3. Track A — blueprint strength (cheap, proof-compatible, CPU-only)

### 3.1 CFR+ shaping refinements — mostly [ALREADY DONE], confirm the missing piece [S]

Reading `table.rs` directly: **CFR+ flooring is already implemented**
(`regret_add_cfr_plus`: `R ← max(R+Δ, 0)`), and **DCFR-style discounting is
already implemented and wired** (`regret_add_cfr_plus_discounted` /
`_slot`: `R ← max(0, R·discount + Δ)`, used by `DeltaBuffer::flush_with_discount`
whenever `trainer.rs` configures `regret_discount < 1.0`). `docs/backlog/perf.md`
confirms: **`[DONE] B-5. DCFR-style regret discounting`**.

What v3-brainstorm's A1 actually asks for that *isn't* yet confirmed done:

- **Alternating updates** (update player 0's regrets on even iterations,
  player 1's on odd) — **[VERIFY]** against `traversal.rs`; not visible in
  the table/arena layer, would live in the traversal loop.
- **Per-street exploitability reporting** (A1's metric) — depends on §2.3
  existing first.
- **The α (positive-regret discount) vs γ (strategy-sum weight discount)
  split** Brown & Sandholm's DCFR actually specifies three independent
  exponents; the current `discount` parameter applies uniformly to
  accumulated regret (which, since it's CFR+-floored, is effectively the α
  knob already). The γ knob (strategy-sum weighting) is separate from
  `averaging_weight()`'s linear averaging — **[VERIFY]** whether these are
  currently coupled or independently tunable in `trainer.rs`'s config.

**Action:** one `EXP-011`-class sweep (already named in v2-roadmap §2) over
`{α, γ}` independently, gated by the §2.3 benchmark, not a days-scale ladder.
This is cheap because the machinery already exists — it's a config sweep,
not new code.

### 3.2 `RegretTable` slot-finding: SwissTable-style probing [M]

`table.rs::RegretTable::find` / `entry_or_insert` do naive linear probing
with 70%-load-factor doubling (`hash_key` then `while slots[i].key != 0 { i += 1 }`).
This is fine at small scale but at 200bb-depth table sizes (tens of millions
of infosets, per the memory-fence math in the brainstorm) naive linear
probing has unbounded worst-case probe-chain length near load 0.7, and it's
exactly the "memory-bound" workload the module's own doc comments flag.

`hashbrown` is **already a workspace dependency indirectly** (it's what
`std::HashMap` uses internally) — using it directly for the slot table means
whitelisting one more direct dep (small, well-audited, SIMD-probed SwissTable,
same design `std::HashMap` already trusts). Concretely:

```rust
// crates/cham-blueprint/src/table.rs — replace the `slots: Vec<Slot>` /
// linear-probe find/insert with hashbrown's raw table API, keeping the
// EXACT same external behavior (find-or-insert-by-key → (off, w)):

use hashbrown::raw::RawTable;

pub struct RegretTable {
    slots: RawTable<Slot>,     // was: Vec<Slot> + manual mask/probe
    n: usize,
    arena: Arena,
    arena_len: u32,
    pub mode: ThreadMode,
    pub renorm_events: u64,
}

impl RegretTable {
    pub fn find(&self, key: u64) -> Option<u32> {
        if key == 0 { return None; }
        self.slots.get(hash_key64(key), |s: &Slot| s.key == key).map(|s| s.off)
    }

    pub fn entry_or_insert(&mut self, key: u64, w: usize) -> (u32, usize) {
        debug_assert!((1..=12).contains(&w));
        if let Some(off) = self.find(key) { return (off, w); }
        let row = 2 * w as u32 + ROW_META;
        let off = self.arena_len;
        self.arena_len += row;
        self.arena.ensure_len(self.arena_len as usize);   // was manual push loop
        self.slots.insert(hash_key64(key), Slot { key, off, w: w as u8 }, |s| hash_key64(s.key));
        self.n += 1;
        (off, w)
    }
}
```

This removes the hand-rolled `grow()` rehash and the manual `mask`
bookkeeping entirely — `RawTable` handles growth, SIMD probing, and tombstone-free
open addressing internally, and is exactly what `std::HashMap` already does
under the hood, so it's not a new trust surface, just an unwrapped one.

*Constraint check:* `hashbrown` has no `unsafe_code` implications for
*your* crates (`#![forbid(unsafe_code)]` covers `cham-blueprint` itself, not
its deps — same note already on record for `memmap2` in v2-roadmap §4.3).
*Kill criterion:* measure directly against `benches/mccfr.rs` at the 200bb
abstraction scale; if the naive linear probe isn't actually the bottleneck
(likely if the workload is memory-bound on the *arena*, not the slot table),
drop it — this is explicitly a "cheap to test, easy kill criterion" item per
`optims-claude.md`.

### 3.3 Evaluator hot path: use the multiset-rank table you already built [S]

`cham-core/src/eval/mod.rs` builds `seven_multiset_ranks: Vec<u32>` (50,388
entries) as a direct reindexing of `seven_map`, currently used **only** by
the GPU/WGSL backend (`eval_tables()`) because WGSL can't do u64 hash keys.
The CPU hot path (`evaluate7`'s non-flush branch, ~97% of calls) still pays
a 7-multiply prime product + a splitmix hash + a linear-probe chain through
`seven_map.get(prod)`.

`docs/backlog/perf.md` marks the naive version of this idea **`[DEAD]`**
correctly — *"incremental multiset rank while scanning is mathematically
wrong"* (combinadic rank needs sorted per-rank counts, not an
online scan). But the reality-checked version in that same entry is right
and just needs a lookup table to be fast:

```rust
// crates/cham-core/src/eval/mod.rs
// Precompute C(n, k) for n in 0..=19, k in 0..=8 once (152 u32s — trivial
// vs the 50,388-entry table it feeds). This turns multiset_rank from
// "recompute n_choose_k_u32 on the fly per card" (up to ~7 nested
// multiply/divide loops) into pure table lookups + adds.
const NCK: [[u32; 8]; 20] = build_nck_table(); // const-eval, or OnceLock if const-eval is awkward here

const fn build_nck_table() -> [[u32; 8]; 20] {
    let mut t = [[0u32; 8]; 20];
    let mut n = 0;
    while n < 20 {
        t[n][0] = 1;
        let mut k = 1;
        while k <= 7 && k <= n {
            // Pascal's triangle recurrence — const-fn friendly, no division.
            t[n][k] = if k <= n { t[n - 1][k - 1] + if n >= 1 { t[n - 1][k] } else { 0 } } else { 0 };
            k += 1;
        }
        n += 1;
    }
    t
}

#[inline]
fn multiset_rank_fast(counts: &[u8; 13]) -> u32 {
    let mut rank = 0u32;
    let mut n = 0u32;
    for i in 0..13u32 {
        for _ in 0..counts[i as usize] {
            rank += NCK[(i + n) as usize][(n + 1) as usize]; // was n_choose_k_u32(i+n, n+1)
            n += 1;
        }
    }
    rank
}

// evaluate7's non-flush return becomes:
//   let mut counts = [0u8; 13];
//   for i in 0..7 { counts[(c[i].0 >> 2) as usize] += 1; }   // built during the
//                                                              // same single pass
//                                                              // that already
//                                                              // builds suit_mask
//   t.seven_multiset_ranks[multiset_rank_fast(&counts) as usize] as u16
// replacing: t.seven_map.get(prod)
```

This removes the u64 multiply-chain, the splitmix finalizer, and the
probabilistic-length linear-probe chain, replacing them with ~7 table
lookups + adds and one guaranteed array index. It is **bit-exact by
construction** — `seven_multiset_ranks` is already asserted (at table-build
time) to be a pure reindexing of `seven_map`, and this same equivalence is
already relied on for GPU/CPU consistency (`consistency_eval7`, P7 gate). No
abstraction change, no memory growth (the table's the same size either way),
zero risk to the memory fence.

*Kill criterion:* measure in `benches/eval.rs` against the current
`eval_evaluate7` baseline (currently **29.58–31.86 µs / 1000 evals**,
P1 gate at 10 µs / 3× over). If table-lookup-with-more-instructions loses to
multiply-chain-with-hash on the M1's actual cache behavior, keep the
current path — this is exactly the kind of thing that needs measuring, not
assuming (per the same backlog entry's own caution).

### 3.4 Hogwild hot-node contention + Snapbatch bit-exact ordering [M, research-flavored — only relevant once Hogwild/multi-iteration training exists]

`docs/backlog/perf.md` correctly marks the naive versions of both of these
**`[DEAD]` / NOT APPLICABLE today** — `trainer.rs`'s iteration loop is
`for t in 0..cfg.iters`, fully serial, single writer to the arena. There is
no CAS contention and no worker-interleaving nondeterminism *yet*, because
there's no parallel-iteration mode yet. This means:

- **`optims-claude.md` items #2 (hot-node CAS contention) and #3
  (Snapbatch bit-exact multi-thread ordering) are premature** in the current
  architecture — they describe problems that would exist *if* a
  parallel-iterations training mode were added, which per B-3/B-4 in the
  backlog it currently isn't. `ThreadMode::Hogwild`/`Snapbatch` today select
  *how a single-threaded write reaches the arena*, not concurrent writers.
- **If/when** a genuine parallel-iteration mode is added (multiple
  traversals in flight at once, e.g. to use the M1's 8 cores for wall-clock
  during A1's iteration-count increase), *then* both of these become live
  concerns, in this order: first the Snapbatch fixed-worker-index-order
  barrier (turns "fast but stochastic" into "fast and bit-exact" — a
  correctness upgrade, cheap once workers exist), then hot-node thread-local
  accumulation for the always-hit preflop-root nodes (a targeted
  micro-optimization, only worth measuring once contention is real and
  visible in `benches/mccfr.rs`).

**Recommendation:** don't build either yet. If A1's iteration-count scaling
(§6 below) turns out to be wall-clock-bound rather than
exploitability-plateau-bound, *that's* the trigger to open a
parallel-iterations design — and when that design doc gets written, these
two items are pre-baked, cited, and ready to implement in the order above.

### 3.5 Bet-size abstraction audit (A4) [S–M]

`ArrayVec<LegalAction, 12>` caps the action set. Concrete audit: for each
street, compare the current geometric bet-sizing template against the
measured encode rates (`encode_flop`: 7.09M keys/s, `encode_river`: 5.25M
keys/s — both already comfortably under the P3 gate) and the byte-budget
table from §4.1. Add size variants only where budget allows; gate on the
§2.3 exploitability benchmark. This is scoped exactly as the brainstorm
describes it — nothing to add beyond executing it once §2.3 exists.

---

## 4. Track A2/D1 — GPU-funded abstraction quality (the actual "dramatically increase competitiveness" lever)

This is the single biggest lever in the whole document, and it's already
de-risked: the GPU table factory is **proven** (worklog G1.2/G4.0: 1.44 GB
of bit-exact turn EHS tables in 76.4 min at 3.57e9 evals/s; flop table in
similar shape at 2.65e9 evals/s; `verify --gpu` P7 24/24 bit-equal). The open
problem, per `worklog.md`'s own G2.0 audit, is that **these tables currently
have no CPU consumer** — every existing candidate (`archetype::ehs`,
`family_b::ehs`, `cham-engine::build::histo_*`) is a deliberate *proxy*, and
substituting exact GPU EHS would change the data-generating process, not
just accelerate it. That audit is correct and I'm not overturning it.

### 4.1 What the GPU tables are actually for: EMD-based potential-aware bucket rebuild [L]

`v3-brainstorm.md` A2 says "richer per-hand features... at the same table
byte budget" without naming the algorithm; `optims-claude.md` #6 names it
correctly: **Earth Mover's Distance over per-hand next-street equity
histograms** (Johanson et al.; Ganzfried & Sandholm), not mean EHS and not
raw equity-variance (a weaker signal). Concretely:

1. **Histogram generation is exactly the GPU factory's proven job.** For
   each flop/turn hand, compute a fixed-bin (e.g. 16-bin, matching the
   existing `histo_*` CDF convention in `cham-engine::build`) histogram of
   equity against the next street — this is bulk enumeration, the same
   shape as the turn/flop EHS builds that already hit 2.65e9–3.57e9 evals/s.
2. **EMD between two sorted 1-D histograms is O(bins)** — it's the L1
   distance between cumulative distributions, cheap on CPU *or* GPU once the
   histograms exist. This is what actually needs to replace Euclidean
   distance in the k-means step (`tables/build.rs`'s existing seeded
   k-means++, per `SPECS/02`).
3. **Byte budget is unchanged**: same bucket *count* (flop k=300, turn
   k=200 per `SPECS/02`), same table shape — only the clustering metric and
   the feature richness feeding it changes. This is the "free strength" the
   brainstorm promises: identical decision latency, identical memory, lower
   abstraction loss, because it's a *better partition of the same space*.

*Kill criterion (A2, as stated in the brainstorm):* if abstraction-local
exploitability (§2.3 benchmark) improves <10% on sampled subgames after the
rebuild, the feature set was already adequate — stop here, don't chase
further feature richness.

### 4.2 GPU jobs ranked (Track D1, unchanged from the brainstorm, now with a consumer identified)

1. **A2 abstraction rebuild** (above) — now has a real consumer, promoted
   from "proven pipeline, no target" to the top of the queue.
2. **Best-response/exploitability sweeps** for §2.3 — embarrassingly
   parallel across sampled subgames, same pipeline shape.
3. **Preflop all-in completion table** for §2.1 AIVAT (1.7M combos, ~0.5 µs
   at steady-state GPU rate) — small, cheap, immediately useful.
4. **Experimental only:** seed-partitioned parallel ES-MCCFR traversals with
   fixed-order reduction. This is the only GPU job that touches training
   math directly, and per Constitution D2 it must clear byte-exact CPU
   validation before any artifact it produces is trusted — same bar as the
   G1.2 precedent, no exceptions.

The determinism fence (D2) stays exactly as specified: GPU output enters
`artifacts/` only after byte-exact CPU validation; the online decision path
stays scalar CPU forever; any kernel that can't be validated bit-exact is
rejected regardless of speed.

---

## 5. Track B — exploitation, and Track A3 — turn solving (sequenced after A2/C, per the brainstorm's own reasoning)

I'm not re-deriving B1 (parametric opponent grid), B2 (router maturity), B3
(LAG task force), or B4 (in-hand reweighting) — the brainstorm's grounding
and kill criteria for these are sound and don't change based on anything I
found in the source. Two additions from `optims-claude.md` that sharpen
items the brainstorm gestures at without fully specifying:

### 5.1 Multiple leaf continuation strategies for A3 turn solving [M, when A3 starts]

DeepStack's actual lesson (not just "solve deeper"): a single fixed leaf
continuation strategy is itself exploitable. `cham-search/src/prior.rs`'s
visit-confidence flattening already handles *unvisited* paths gracefully —
this is a different bug class (a *confident but wrong* leaf strategy). When
A3 extends solving to turn subgames, add 2–3 leaf variants (perturbations of
the blueprint prior — e.g. a call-heavy and a fold-heavy variant alongside
the base prior) to the turn solver's leaf set, blended by a small combinator
solved *within* the subgame. This is bounded, offline-computable (the
perturbed priors are precomputed, not learned online), and doesn't touch the
online decision path's determinism — it only gives the solver more to be
robust against at the leaves.

### 5.2 Bayesian sequential router update, replacing the fixed-α smoother [M]

`cham-router/src/runtime.rs::weights_for_hand` blends via a fixed-α
exponential smoother — ad hoc. Replace with a Beta-Binomial posterior per
specialist over the session's observed classification features, combined
with the softmax prior by **adding log-likelihoods** (proper Bayesian
fusion) instead of linearly blending probabilities. This directly produces a
real posterior variance usable as B2's confidence-gate threshold (currently
presumably a tuned constant — **[VERIFY]**), converges faster on strong
early evidence, and degrades gracefully on weak evidence — exactly the
property B2 is reaching for by hand. Note this **supersedes** v2-roadmap's
"skip Dirichlet-posterior router confidence" verdict only in scope, not
conclusion: v2 correctly skipped the *specific* unverifiable "Nov 2025
lecture" citation and noted the existing `c = v/(v+64)` visit-counter is
already shape-equivalent for *that* use case. Beta-Binomial fusion here is a
different, better-grounded target (B2's *confidence gate*, not the
visit-counter), so it's not the same idea reappearing — it's filling a gap
the visit-counter was never meant to cover.

---

## 6. M6 — Self-Exploit Audit (v2's headline idea; do this once artifacts exist)

`docs/plans/v2-roadmap.md §1.1` already scopes this well and it doesn't need
re-deriving: wrap a frozen `full`-agent snapshot as an `OpponentSpec::Frozen`
analytic opponent (same shape as `ArchetypeAgent`'s `action_probs`), point
`cham-blueprint`'s existing `TrainMode::Exploit` at it, and measure
`G-SELF`. The **adaptive** variant (real tracker/router across a session,
so the trained exploiter can try to manipulate the classifier — e.g. play
nit for 40 hands to get routed into the nit-specialist mixture, then
deviate) is, per that doc, *"the single biggest unaddressed risk in a
router-based architecture"* and it's answerable with tooling that already
exists. One scheduling note tying it to this roadmap: **run this after §5.2's
Bayesian router update**, not before — the adaptive self-exploiter is
specifically probing router confidence dynamics, and testing it against the
ad hoc fixed-α smoother produces a number you'd have to re-measure anyway
once the smoother changes.

---

## 7. Sequencing (EV per unit effort, combining all tracks above)

| Order | Item | Files | Cost | Why here |
|---|---|---|---|---|
| 1 | §1.1 `ab.rs` parallel/isolated `run_shared` | `cham-eval/src/ab.rs`, `cham-cli/src/cmd/ab.rs` | S | Bug fix + makes every later item's iteration loop 4–8× faster. Do this literally first. |
| 2 | §1.2 cache LRU + pre-warm | `cham-search/src/cache.rs` | S | Compounds with #1 for A/B iteration speed. |
| 3 | §2.1 step 1, §2.2, §2.3 | `cham-eval/src/vr.rs`, new `cmd/lint_ledger.rs`, new `benches/exploitability.rs` | S–M | Makes every gate below decidable; brainstorm's own top priority. |
| 4 | §3.3 evaluator table swap | `cham-core/src/eval/mod.rs` | S | Free, bit-exact, measurable in minutes; do opportunistically. |
| 5 | §3.2 SwissTable RegretTable | `cham-blueprint/src/table.rs` | M | Only matters at 200bb scale; measure after Step Zero lands real artifacts. |
| 6 | §3.1 DCFR α/γ sweep + alternating updates `[VERIFY]` | `cham-blueprint/src/trainer.rs`, `traversal.rs` | S–M | Cheap, proof-compatible, reads off #3's benchmark. |
| 7 | §4.1 EMD bucket rebuild (A2) | `cham-engine/src/build.rs`, `cham-gpu/*` | L | The actual "dramatically better" lever; needs #3's telemetry to know if it worked. |
| 8 | §5.1/§5.2 B1/B2 + leaf variants + Bayesian router | `cham-search/src/prior.rs`, `cham-router/src/runtime.rs` | M–L | Exploitation program; needs #7's blueprint to specialize against. |
| 9 | A3 turn solving | `cham-search/src/{solve,subgame,trigger}.rs` | L | Last in the training arc — solving quality inherits abstraction quality. |
| 10 | §6 M6 Self-Exploit Audit | new `cham-audit` or `cham-eval` | M | Needs a stable router (post #8) to produce a meaningful adaptive number. |

This preserves `v3-brainstorm.md §6`'s through-line — **C-track first, A-track
second, B-track third, A3 last** — while inserting the A/B-speed fix at the
very front, because it's a free multiplier on every subsequent
iteration's cost, and inserting the "already done" corrections (§3.1, §3.4)
so effort isn't spent re-discovering work that's already landed.

---

## 8. What's already shipped — don't re-do it

Confirmed **[ALREADY DONE]** by reading the code, not just the backlog:

- CFR+ regret flooring (`table.rs::regret_add_cfr_plus`).
- DCFR-style discounted regret (`table.rs::regret_add_cfr_plus_discounted*`,
  wired through `DeltaBuffer::flush_with_discount`).
- Postcard migration stage 1 (`cache_persist.rs`, VERSION 1→2).
- River-subgame cache: in-process L1 (`cache.rs`) *and* cross-session
  persistence (`cache_persist.rs`, wired into `play`/`ladder`/`ab` via
  `cmd/cache_guard.rs`).
- `ladder.rs`'s per-opponent parallel + SPRT-chunked real-hero evaluation
  (the pattern §1.1 ports into `ab.rs`).
- GPU turn + flop EHS tables, bit-exact-validated, `wgpu`/Metal dual backend
  with WGSL portability path.
- B4 all-in-EV variance reduction (`vr.rs::allin_ev_adjusted`, wired into
  `matcheng.rs`).
- `on_public_action` wiring fix (the bug that caused 65%→0% ladder fallback,
  per worklog G4.0).

Do **not** schedule work against B-3/B-4 in `docs/backlog/perf.md` (Hogwild
CAS contention, Snapbatch multi-thread ordering) until a genuine
parallel-iterations training mode exists — see §3.4.

---

## 9. Guardrails this roadmap inherits without modification

Everything above stays inside the existing constitution:
no `unsafe` in core crates, no inference-time neural network, bit-exact
determinism for every artifact that enters `artifacts/`, the 16 GB memory
fence with a byte-budget table for any abstraction growth, the 150 ms p99
self-cap on expanded solving, and Slumbot staying a diagnostic anchor, never
a promotion gate. Every item has a stated kill criterion above; if a kill
criterion fires, stop and redirect budget per that item's note — don't
silently keep funding something the project's own measurement gates say
isn't working.
