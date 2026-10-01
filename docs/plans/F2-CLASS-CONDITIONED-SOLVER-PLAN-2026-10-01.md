# F2: class-conditioned solver rewrite — plan (2026-10-01)

## Confirmed state

`crates/cham-search/src/solve.rs`:

- `cfr_plus` regrets keyed by `(String path, u8 seat)` — NOT by own class.
  Line 399: `let mut regret: BTreeMap<(String, u8), Vec<f64>> = BTreeMap::new();`
- `strat_sum` keyed the same way. Line 420.
- `avg_ev_child` (line 533) weights villain classes by their UNCONDITIONAL
  prior at every node: `total += hc.weight * vc.weight * ev(...)`.

Two consequences, both real:

1. **σ(path) is shared across hero's own strength classes.** The solver
   plays the same mix for the nuts and for a bluff-catcher. This is not a
   poker strategy.
2. **Villain ranges never narrow along a path.** On a path where villain
   bet + raised, the correct villain class distribution is
   `prior × Π σ_villain(a | path', class)`, i.e. counterfactual reach.
   The unconditional prior is used instead, so value/bluff thresholds are
   computed against the wrong ranges at every non-root node.

## The rewrite (sketch)

### Data structures

    type CKey = (String /*path*/, u8 /*player*/, usize /*own class*/);
    let mut regret:    BTreeMap<CKey, Vec<f64>> = BTreeMap::new();
    let mut strat_sum: BTreeMap<CKey, Vec<f64>> = BTreeMap::new();

### Per-iteration reach computation (once per iter, O(nodes × classes))

Add two reach tables computed from the current `effective` strategies:

    // hero reach: hero's own class c reach = prior_h(c) × Π_{hero actions on path} σ_h(a | path', c)
    let mut reach_h: HashMap<(String, usize), f64> = ...;
    // villain reach: villain's class o reach = prior_v(o) × Π_{villain actions on path} σ_v(a | path', o)
    let mut reach_v: HashMap<(String, usize), f64> = ...;

Compute recursively down the tree: at each node with player p and path
`path`, the child paths extend `reach_p` by the action's probability for
each class of that player. Start both at the root with `prior_h(c)` and
`prior_v(o)`.

### CFR+ update

For each node (path, player, actions) and for each own-class `c` of that
player:

    v[a] = Σ_{o in opponent_classes} reach_opp(path, o) × ev_fixed_class(child_a, player, c, o)

    node_v = Σ_a σ(path, player, c)[a] × v[a]

    regret[(path, player, c)][a] += (v[a] − node_v)      # RM+ floors at 0
    strat_sum[(path, player, c)][a] += w_t × σ(path, player, c)[a]

Note: the counterfactual reach for regret weighting is the OPPONENT's
reach (standard CFR). Since we already weight `v[a]` by `reach_opp` above,
the regret update is `v[a] − node_v` (no extra factor) — this is correct
because CFR's counterfactual value already includes the opponent's reach
in the sum over their classes.

### Deployment

At the decision point, hero knows its own class `c_h` (from the encoder's
river-equity rank). Query `σ(path, 0, c_h)`. Villain's strategy, when
queried, is `σ(path, 1, c_v)` for whatever class the villain has in the
simulation.

### B6 warm-start

The `warm` table currently maps `String → Vec<f64>`. Extend to
`String → Vec<Vec<f64>>` (one per class) or, simpler, treat the warm
strategy as class-uniform and assign it to every class. Preserve the
existing callers' semantics.

### RNR villain override

`villain_override` currently maps `String → Vec<f64>`. Same extension
as warm-start: either class-uniform or per-class. The blend is per
(path, villain class) after the extension.

## Acceptance test (Kuhn-style one-card river)

A 1-card river with 2 hero classes and 2 villain classes, 2 actions
(check/bet), 1 bet size, hand-computed Nash value. The current
shared-strategy solver CANNOT pass this (its σ mixes the two hero
classes into one distribution); the class-conditioned one must.

Add as `crates/cham-search/tests/one_card_poker.rs`:

    // (sketch)
    // Hero classes: [strong (nuts, weight 0.5), weak (bluff-catcher, weight 0.5)]
    // Villain classes: [bluff (weight 0.5), value (weight 0.5)]
    // Pot = 1, stack = 1, bet_fracs = [1.0]
    //
    // Compute expected value of the class-conditioned Nash equilibrium:
    //   hero value = ...
    //   hero bluff frequency = ...
    // Assert solver output within ±1e-3.

The exact hand math will be written when the rewrite lands.

## Effort

- Data structure + loop rewrite: ~2 days careful work
- Reach computation: ~half day
- Warm-start + override extension: ~half day
- Kuhn fixture: ~half day
- Total: ~3 days

## Why this is not being done today

The existing solver is DEAD CODE on the live decision path (F1).
Until F1 wires it in, no user-visible behavior depends on it. The
correct sequence is:

1. **F2 first** (this plan) — a correct solver
2. **Then F1** — wire the correct solver into the pipeline
3. **Then** measure search impact on the ladder

Doing F1 first would deploy the wrong-game solver, then require
re-measuring everything after F2 lands.

## Related

- `COMPETITIVE-REVIEW-2026-10-01.md` — the review that flagged F2
- `crates/cham-search/src/solve.rs` — where the rewrite lands
- `docs/plans/20260927-solver-c234-pending.md` — the earlier solver work
