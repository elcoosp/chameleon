# F10: vector-form river solver — plan (2026-10-02)

## What the report says

> `cham-search` collapses ranges to **weighted 1-D strength classes**.
> That choice makes testing easy, but it throws away card removal and
> the multi-dimensional structure of real ranges, and it is not
> safe-resolving (no gadget, no blueprint-CFV constraint).

> With <=1326 combos per player and ~20-50 nodes, one CFR+ iteration
> is a few x10^5 flops, well within a 250 ms clock for several hundred
> iterations. The one non-trivial piece is the showdown utility with
> blockers in O(n) rather than O(n^2). I validated this kernel in
> Python against brute force (1,081 combos with many ties, max error
> 5.7e-13).

## Why this is not a one-day job

The current solver (`crates/cham-search/src/solve.rs`) is a class-
conditioned CFR+ over a strength-class abstraction. The F10 rewrite
replaces the entire inner loop with a **vector-form** CFR+ over full
combo ranges, adds the O(n) showdown kernel with card removal, and
adds the safe re-solving gadget.

That is 1-2 weeks of focused work per the report's own estimate.

## The kernel (ready to port)

The report includes the O(n) showdown CFV algorithm:

    pub fn showdown_cfv(
        hands: &[[u8; 2]],
        rank: &[u32],
        opp_reach: &[f64],
        out: &mut [f64],
    )

With `rank` ascending (ties equal), it computes for each hand i:

    out[i] = Σ_j opp_reach[j] × sign(rank_i − rank_j) × [no card overlap]

in O(n) time using a running "below" sum and a 52-slot per-card
counter. Card overlap is handled by inclusion-exclusion: subtract the
mass of hands sharing card a or card b, add back the mass sharing both
(one specific hand).

## The vector CFR+ outline

For each node in the river tree:
- `regret[action][hand]` — per-combo regret
- `strat[action][hand]` — per-combo strategy sum
- Pass **reach vectors** down (one per combo), CFV vectors up.
- Hero node: `v[action][hand] = Σ_a σ[action][hand] × v_child[action][hand]`
- Villain node: sum of children weighted by villain's reach.
- Terminal:
  - Showdown → `showdown_cfv(...)` kernel
  - Fold → `Σ_j reach[j] × [disjoint]` by inclusion-exclusion

## The safe re-solving gadget

Replace "ranges from blueprint reach, flattened" with the Burch/
Brown-Sandholm gadget: at the subgame root, the opponent chooses per
hand **Follow** (enter the subgame) or **Terminate** and receive the
blueprint's counterfactual value v_bp(h) for that hand. Then the solved
subgame guarantees the opponent gets at most v_bp(h) per hand, so the
combined strategy is no more exploitable than the blueprint (up to
approximation error in v_bp).

On the river, v_bp can be computed exactly by evaluating the
blueprint's strategies over the same tree (1326 key lookups per node,
a few ms).

## Order of work

1. **Vector kernel + tests — DONE (2026-10-02).**
   `crates/cham-search/src/kernel.rs` now has `showdown_cfv`
   (symmetric), `showdown_cfv_two` (hero vs villain ranges, overlapping
   pools), and `fold_cfv`. Validated against O(n^2) brute force in
   `crates/cham-search/tests/kernel_bruteforce.rs` (8 tests: many-ties,
   no-ties, all-ties, empty, plus the two-range variants). Note: the
   symmetric form caps at 26 card-disjoint hands (52-card deck); the
   two-range form is the one the river CFR+ rewrite should use.
   Commits: `676d2fe` `d8fed5e` `3cffbe1` `ae6b576` `d89c5f3` `5be30c7`
   `d0a5288`.
2. **Vector CFR+ on the river.** Rewrite `cfr_plus` to the vector form.
   Keep the existing class-conditioned solver for the oracle test.
   ~3-4 days.
3. **Unsafe re-solve (no gadget).** Wire the vector solver into the
   subgame without the safe constraint. Measure whether it beats
   blueprint-only play in duplicate. ~1-2 days.
4. **Safe gadget.** Add the Follow/Terminate root choice with the
   blueprint CFVs. Re-measure. ~2-3 days.
5. **Turn solving.** Same machinery extended to turn, with the exact
   river solve (or a river CFV table) as the depth-limit leaf. ~3-4 days.

## What has been done

**Step 1 (kernel + tests): DONE 2026-10-02.** See the Order of work
above. Remaining steps 2-5 are still multi-day work; the next session
continues at step 2 (the vector CFR+ rewrite of `cfr_plus`).

## Prerequisites already in place

- `cham-search` compiles with the F2 class-conditioned solver.
- The `Subgame::tree()` layout is stable and testable.
- The `SolveResult` type has the class-conditioned fields
  (`our_class_strategy`, `their_class_strategy`) that the vector form
  can be extended alongside.
- The one-card-poker acceptance test (`crates/cham-search/tests/one_card_poker.rs`)
  validates the class-conditioned solver; a parallel vector-solver test
  can reuse the same spot.

## Related

- `chameleon-competitiveness-report.md` — F10
- `F2-COMPLETE-2026-10-01.md` — the class-conditioned solver
- `F1-SEARCH-CORRECTED-2026-10-01.md` — the reason F10 matters (search is
  still net negative on the scripted pool)
