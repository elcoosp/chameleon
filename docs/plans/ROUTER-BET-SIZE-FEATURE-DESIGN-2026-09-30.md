# Router bet-size histogram feature — design (2026-09-30)

## The problem

The 11-dim honest router (10 raw frequencies + preflop/postflop tilt)
failed the gate (`ROUTER-11DIM-NEGATIVE-2026-09-30.md`). The raw
frequency approach cannot separate TAG from LAG because:

- Both archetypes raise a lot preflop and fold a lot to 3bets.
- The DIRECTION of their aggression across streets is opposite, but
  the ratio scalar I added didn't capture this in practice (real
  hands, hero-policy coupling, small effect).

The remaining signal is **which hands the opponent raises with**, not
how often. Two paths:
1. Showdown-strength distribution (needs I9 leak-rule audit).
2. **Bet-size histogram** (this design).

## Why bet size is the signal

From `crates/cham-opponents/src/params.rs::point()`:

| archetype | size_idx | sizing fraction |
|---|---|---|
| Nit      | 1 | 66% pot |
| **Tag**  | 1 | **66% pot** |
| **Lag**  | 2 | **100% pot** |
| Station  | 0 | 33% pot |

TAG bets 66% pot; LAG bets 100% pot. This is a **categorical
difference** that a histogram captures directly. All postflop bet
and raise sizes use `SIZE_FRACS[size_idx]` in `archetype.rs`, so the
signal is present in every postflop aggression the opponent makes.

## I9 compliance

The tracker's input is `&PublicHistory` only. `PublicHistory.actions`
is `Vec<(Street, Player, Action)>`, and `Action::Bet { to }` /
`Action::Raise { to }` already carry the public chip amount. Bet
sizes ARE public information (they're what everyone at the table
sees). No I9 violation.

## The proposed feature

Add a per-street bet-size histogram to the `Tracker`:

    pub opp_postflop_aggression_by_size: [u64; 4],   // buckets by SPR-pot fraction

with 4 buckets matching the archetype design:
- bucket 0: bet/raise ≤ 40% pot (station-like)
- bucket 1: 40-80% pot (tag-like)
- bucket 2: 80-120% pot (lag-like)
- bucket 3: >120% pot (overbet)

Or, simpler and more general, an 8-bucket distribution aggregated
across all postflop streets. The router takes the normalized
histogram (8 values) as features.

## Why 8 buckets not 4

The archetype params give 3 fractions (0.33, 0.66, 1.0) but real
opponents (perturbed variants) have jitter on `size_idx` and on
`river_bet_fracs`. 8 buckets over [0, 2.0] gives 0.25-wide bins:

    [0, 0.25), [0.25, 0.5), [0.5, 0.75), [0.75, 1.0),
    [1.0, 1.25), [1.25, 1.5), [1.5, 1.75), [1.75, +∞)

This resolves the 0.33 / 0.66 / 1.0 clusters without oversmoothing.

## The bucket fraction definition

    frac = (bet_to - street_bet[actor] - last_level) / pot_before_bet

Where `street_bet[actor]` is the actor's contribution so far on this
street and `last_level` is the current bet level. That gives the
*incremental* bet as a fraction of the pot. The tracker already
computes `increment` on line 189 of `tracker.rs`:

    let increment = *to - street_bet[p] - last_level.max(0);

So the amount is already in scope; we just need to divide by the
pot at the time (also in scope: `ph.pot` isn't in PublicHistory
directly but can be recomputed from the action sequence + blinds +
antes; alternative is to add a pot field).

Actually `PublicHistory` doesn't carry pot directly. So we need
either:
(a) recompute pot from actions (blinds are config-known; BB/SB are 100/50 chips)
(b) add a pot field to PublicHistory

Option (a) is trivial: track pot as we walk the actions (call adds
`to_call`, raise adds increment, etc.). The tracker already does
similar level tracking.

## Impact and integration

Files to change:

1. `crates/cham-agent/src/tracker.rs`
   - new field: `opp_postflop_bet_size_hist: [u64; 8]`
   - update in the existing `Action::Bet | Action::Raise` branch of
     `observe_hand`
   - new accessor: `pub fn opponent_bet_size_hist(&self) -> [f64; 8]`
     (normalized)

2. `crates/cham-agent/src/pipeline.rs`
   - new accessor: `opponent_only_features_19` (10 raw + 8 bet-size
     histogram + 1 tilt = 19 dims)
   - OR a separate accessor keeping the 11-dim stable

3. `crates/cham-cli/src/cmd/collect.rs`
   - new flag: `--raw-opponent-19`
   - emit `hero.opponent_only_features_19()`

4. `crates/cham-router/src/model.rs`
   - no change (SoftmaxModel is dimension-agnostic)

5. `crates/cham-router/src/features.rs`
   - no change (this is a separate feature vector, not the 20-dim contract)

## Cost

- Tracker change: ~30 lines + tests
- Pipeline accessor: ~10 lines
- collect flag: ~10 lines
- Data collection: 10 min (same as 11-dim run)
- Router train: ~5 min
- Total: ~30 min of work + 15 min of wall

## The success criterion

If the 19-dim router's gate (top-1 ≥ 0.80, per-class recall ≥ 0.70,
ECE ≤ 0.15) passes, install it in a new `agent-honest-19dim` bundle
and measure the mixture ladder. If the ladder improves on the
+4 388 baseline, the mixture path becomes viable; if not, the
mixture architecture itself needs rethinking.

## Why this is the right next step

1. **It's the last cheap lever on the router.** After this, only
   showdown-strength distribution remains, which requires an I9
   audit.
2. **It directly targets the failure mode.** The 11-dim negative
   showed that aggregate frequencies don't work. Bet sizes are the
   most direct "which hands" signal available from public history.
3. **The signal magnitude is large.** TAG 66% vs LAG 100% is a
   34-percentage-point gap. Even with jitter, this should be
   separable.

## Related

- `ROUTER-11DIM-NEGATIVE-2026-09-30.md` — the negative that motivates this
- `ROUTER-TILT-FEATURE-DESIGN-2026-09-30.md` — the previous attempt
- `ROUTER-FEATURE-LEAK-2026-09-29.md` — the original leak finding
- `crates/cham-opponents/src/archetype.rs` — where SIZE_FRACS is used
- `crates/cham-opponents/src/params.rs` — the point parameters table
