# F1 villain range upgrade — design (2026-10-02)

## The problem

The F1 search bridge currently uses an **agnostic villain range**:
`DEFAULT_VILLAIN_CLASSES = 3` uniform-weight classes spread
uniformly over `[0, 1]`. This is not a model of any opponent. Against
calling-heavy opponents (`callbot`, `arch:station`), the fictional
range is much stronger than the real one, and the search refuses to
value-bet the hero's strong hands. Result: −11 016 mb/seating against
callbot, −6 731 against station (`F1-SEARCH-CORRECTED-2026-10-01.md`).

Fix: build a real villain range from data the agent already has.

## The data available

The `Tracker` (cham-agent/tracker.rs) exposes:

- `raw_opponent_frequencies() -> [f64; 10]`
  - `[0]` preflop raise freq
  - `[1]` preflop call freq
  - `[2]` preflop fold freq
  - `[3]` flop bet freq (of hands that reached flop)
  - `[4]` turn bet freq
  - `[5]` river bet freq
  - `[6]` showdown reach freq
  - `[7]` aggression ratio
  - `[8]` passivity ratio
  - `[9]` limp freq
- `preflop_postflop_tilt() -> f64`
- `opponent_bet_size_hist() -> [f64; 8]`

These are **marginal** frequencies. They do not directly give a
per-strength-class villain range. To get one we need either:

(a) **Reach from the robust blueprint.** The `BlueprintPolicy` holds
    per-infoset regret tables. Given the current public path, the
    robust policy's `strategy(obs, encoder, seq)` returns hero's
    strategy — but the SAME machinery could, in principle, walk the
    villain's rows and produce a per-class reach for villain.

(b) **Showdown-based reconstruction.** When the opponent reaches a
    showdown, log their hole cards. Over many hands, build an
    empirical distribution of `(board_texture, strength)` pairs.
    Requires an I9 leak-rule audit (showdown cards are revealed, so
    it is public information, but exposing them to the tracker
    changes the invariant structure).

(c) **Heuristic mapping from marginals to a distribution.** Given
    `opp_river_bets`, `opp_showdowns`, and the current bet-size
    histogram, sample a small parameterized family of ranges (e.g.
    "tight/medium/loose calling range" templates) and pick the one
    whose marginal statistics best match the tracker. This is the
    cheapest but least principled.

## The recommended approach: (a) reach from the robust blueprint

### Why

- The blueprint already encodes a coherent poker strategy over the
  abstraction. Villain's marginal at any node is the same kind of
  quantity we already compute for hero in `Traversal`.
- No I9 changes required.
- The tracker's frequencies become a *check* on the derived range,
  not the range itself.

### Sketch

1. At the moment of the trigger (hero acts first on the river, pot
   meets minimum), build the subgame as today.
2. Compute the villain reach per class **from the current public
   action sequence**, using the robust policy as the strategy
   oracle:
   - Start with the class prior = `hero_classes` / `villain_classes`
     from the encoder's abstraction for the current board.
   - Walk the public action sequence. On each villain decision,
     multiply the reach vector by `σ_villain(a | class)` from the
     robust policy.
   - The resulting normalized reach vector is the villain range at
     the current node.
3. Feed that range into the subgame (replacing the uniform spread).

The interesting step is (2) — walking the villain's own action
history and re-weighting their classes by the probabilities they
would have played those actions under the robust policy.

### Effort

- 1-2 days of work in `cham-agent` (a new function that walks
  `seq`, calls `robust.strategy(obs_villain, encoder, seq_villain)`,
  and returns the reach vector).
- 1 day of testing (fixtures where the villain's true range is
  known analytically).
- 1 day of A/B ladder measurement.

## The fallback: (c) heuristic templates

If (a) turns out to be too invasive, (c) is a cheap alternative:

- Pre-define K **villain range templates** as distributions over the
  9 hero classes in the abstraction. E.g.:
  - "Tight caller": mass concentrated on classes 7-9 (strong).
  - "Loose caller": mass on classes 0-9, near-uniform.
  - "Bluffer": mass on classes 0-3 + 7-9 (polar).
- Use the tracker's `river_bet_freq`, `showdown_reach_freq`, and
  `aggression` to select a weighted mixture of templates.
- This is a 3-hour job and gives a strictly better model than the
  current uniform spread.

## What NOT to do

- **Do not re-run the A/B on the current model.** Every search
  measurement using the uniform K-class spread is dominated by the
  modelling error, not the search decision.
- **Do not ship `--search`.** It should stay off until the villain
  range beats the current on `callbot` and `station`.

## Order of operations

1. **Measure the current model's villain range directly** — dump
   the subgame range per opponent, compare to the true archetype
   ranges in `params.rs`. Quantify the mismatch. (30 min.)
2. **Implement (c) heuristic templates** — 3 hours. A/B ladder.
3. **If (c) shows real gain**: commit to (a) blueprint-reach. A/B
   again. If (a) is meaningfully better, keep it.
4. **If (c) shows no gain**: search is genuinely not the frontier
   lever on this pool, close the investigation.

## Artifacts

- `crates/cham-agent/src/search_bridge.rs` — the villain-range
  construction (currently the uniform spread)
- `crates/cham-agent/src/tracker.rs` — the frequency accessors
- `crates/cham-opponents/src/params.rs` — the true archetype
  parameter distributions (for step 1 comparison)

## Related

- `F1-SEARCH-CORRECTED-2026-10-01.md` — the A/B that motivated this
- `F2-COMPLETE-2026-10-01.md` — the class-conditioned solver
- `COMPETITIVE-REVIEW-2026-10-01.md` — finding F1
