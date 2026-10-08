# Full-game VBR — design (2026-10-08)

**Purpose.** Settle Decision D1: the shipped blueprint's true
exploitability, measured by an exact card-perfect best response over the
full 4-street game. The river VBR (`crates/cham-search/src/vbr.rs`) gives
5.89 bb; the tabular BR gives 8.19 bb (tiny) / 10.27 bb (rlf-g2). The
full-game number decides whether that gap is a real abstraction leak or a
metric artifact (plan C-2).

## Why the river VBR is not enough

`RiverVbr` fixes the pot at 2 bb and the stack at 98 bb, assumes the
harness line "check/call to river", and explores a fixed river betting
tree. It does not:

- enumerate flop/turn runouts (chance nodes);
- let hero's preflop/flop/turn strategy depend on the runout;
- account for villain reaching the river with a board-dependent range.

So the D1 river number (~5.9 bb for every config) is a lower bound on the
river slice only. Whether the earlier streets add 2 bb or 12 bb is exactly
what the full-game VBR measures.

## The algorithm

Exact BR is a recursion over (public state, hero range, villain reach):

    fn walk(
        st:            State,
        path:          &str,
        hero_range:    &[[u8; 2]],
        hero_w:        &[f64],
        villain_range: &[[u8; 2]],
        villain_reach: &[f64],
    ) -> Vec<f64>
        // Returns EV per hero combo.
        // `villain_reach` is the distribution over villain combos that
        // arrive at this public node; it is a function of villain's
        // strategy on earlier streets and the sampled board.

Cases:

- **Hero to act.** For each legal action `a`, compute
  `v_a = walk(st_a, path_a, hero_range, hero_w, villain_range, villain_reach)`.
  Return `max_a v_a` elementwise (per hero combo).
- **Villain to act.** For each legal action `a`, let
  `r_a = villain_reach * P_villain(a | hand, path, board)`, recurse, and
  return the sum of children.
- **Chance (street ends, board < 5).** For each remaining card `c`,
  remove `c` from `hero_range` and `villain_reach`, recurse with the
  dealt card, and average over `c` with weight 1/N. Bounded by sampling
  (below).
- **Terminal.** Showdown or fold. Showdown uses `showdown_cfv_two`
  (O(n) card-removal-correct EV). Fold uses `fold_cfv`.

Root: `walk(start_state, "", hero_full_range, hero_w, villain_full_range, villain_w)`.

## Sampling

Enumerating all flops C(50,3)=19600, times 47 turns, times 46 rivers is
~4.2e7 boards, each with a betting tree of tens of nodes — intractable.
Instead sample:

- `n_flops` flops, uniform without replacement from C(50,3);
- per flop, `n_turns` turn cards (or all 47 if cheap);
- per turn, `n_rivers` river cards (or all 46).

Estimator mean = true BR, SE = std / sqrt(N). At N = 200 flops x 4 x 4
= 3200 boards and per-board std ~2 bb, SE ~ 0.2 bb, matching the river
harness. Flop / turn / river cards are drawn from disjoint RNG streams
so the estimate is unbiased and reproducible.

## Preflop

Preflop hole cards are dealt by `State::new` from the deck prefix. We do
not sample hole cards; hero/villain ranges are the full combo sets and
card removal is handled by the kernels. The 4-street betting tree is
walked once per sampled board.

## Interface

    pub struct FullGameVbr<'a, F>
    where
        F: FnMut(&Observables<'_>, &str, &[u8; 2]) -> Vec<f64>,
    {
        pub hero_range:    &'a [[u8; 2]],
        pub hero_w:        &'a [f64],
        pub villain_range: &'a [[u8; 2]],
        pub villain_w:     &'a [f64],
        pub hero_seat:     usize,
        pub cfg:           EngineConfig,
        pub bet_fracs:     &'a [f64],
        pub policy:        F,
    }

    pub struct FullGameVbrResult {
        pub mean_bb:  f64,
        pub se_bb:    f64,
        pub n_boards: usize,
    }

Villain's policy is queried per concrete node (the blueprint is keyed on
bucket(cards) and the betting path), not just on the path.

## Reuse

- Kernels `showdown_cfv_two`, `fold_cfv` — unchanged.
- Villain policy query mirrors `crates/cham-agent/tests/d1_vbr_blueprint.rs`:
  build a `State`, record to the current path, call `policy.strategy(&obs, ...)`.
- River subgame inside the walk delegates to `RiverVbr` when both players
  are on the river and the stack is known — a large saving.

## Testing

- **Reduction.** With a "check/call to river" preflop line and no
  flop/turn bets, the full-game VBR must equal the river VBR averaged
  over the same sampled boards.
- **Brute force.** On a shortened-deck toy, compare against a naive
  enumerative BR.
- **Monotone.** Full-game BR >= river-only BR (a wider strategy space
  cannot hurt the best responder).

## What this settles (D1)

- If full-game BR is ~5.9 + small, the blueprint's exploitability is
  river-dominated; the tabular 8.19 / 10.27 is a metric artifact of the
  in-abstraction tabular solver.
- If full-game BR is much larger than 5.9, the abstraction leaks earlier
  streets; Phase C (PCS trainer) must widen the abstraction (W2) before
  training is worth the wall clock.

Either way the number is the D1 gate and unblocks Phase C.
