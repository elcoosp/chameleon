# The honest dataset makes router classification HARDER (2026-09-29)

> **Metric note (2026-10-01):** LBR figures in this doc use the clairvoyant `lbr::lbr_vs`; the corrected infoset-consistent value is 6-10x smaller (`docs/plans/F1-CORRECTED-METRIC-2026-10-01.md`).


## The measurement

After fixing the seat-asymmetry bug (`collect --real` now alternates
hero SB/BB per session), I retrained the router on the corrected 120k-row
dataset. The result is worse:

| metric | biased (seat-fixed=no) | honest (seat-fixed=yes) |
|---|---:|---:|
| top-1 B-dev | 0.761 | **0.649** |
| top-1 B-test | 0.719 | 0.697 |
| ECE B-test | 0.207 | 0.201 |
| recall nit | 0.806 | 0.869 |
| recall tag | 0.453 | **0.375** |
| recall lag | 0.790 | **0.514** |
| recall station | 0.951 | 0.860 |

TAG and LAG both get worse. NIT gets better (it's an extreme
archetype, so the seat alternation didn't hurt it).

## Why the honest data is harder

The seat alternation was correct — it removed a systematic bias. But the
features it turned on are the wrong KIND of features:

1. `opp_faces_open` went from 0.000 to 0.153.
2. `opp_faces_3bet` went from 0.000 to 0.033.
3. `ewm_vpip` went from 0.115 to 0.27.
4. `ewm_pfr` went from 0.115 to 0.23.

The classifier now has more input signal, but the signal is **more
variable** because it now includes the hero's seat identity. Against the
same opponent, hero-at-SB and hero-at-BB produce different tracker
values, because the hero's own policy differs by seat. So the new
variation is:
- **half** opponent signal (the opponent really does play differently
  against a SB-limp vs a BB-limp)
- **half** hero-seat artifact (the tracker's features are computed from
  a stream that includes hero's actions)

This is the same **leak** class as `trend_z`: features that depend on
the (opponent, hero) pair, not on the opponent alone. Adding seat
variation doesn't fix the leak; it just makes the leak bimodal.

## The deeper structural problem

**Every EWM in the tracker is opportunity-gated in a way that depends
on hero's policy.** Examples:

- `EWM_3BET` only updates when hero opens (facing_open = hero raised).
- `EWM_CBET_FLOP` only updates when the opponent is the preflop
  aggressor — which requires hero to have limped or checked.
- `EWM_FOLD_TO_CBET` only updates when hero bet postflop first.

So the value of `EWM_3BET` for the same opponent differs based on the
hero's own tendencies. This is not a tracker bug — the tracker is doing
exactly what the spec said. But it means the resulting 20-dim vector is
not a pure function of the opponent, and a classifier trained on it is
learning (opponent, hero-policy) joint patterns.

## What the numbers say

The best achievable top-1 with the current feature vector, on honest
data, is around 0.65. That is 15 points below the gate (0.80). No
amount of router training over the same features will close that gap,
because the features don't carry the information.

## What this means for the mixture

The mixture's router weights are **routing on a leaky feature**. Any
ladder win the mixture shows is partly from real opponent detection and
partly from memorizing which (opponent, hero-seat) patterns co-occurred
in training. In deployment against a new opponent at a new seat
assignment, the leak doesn't transfer.

Argmax remains the honest default. The mixture's LBR advantage (35%)
comes from the *hedge*, not from the routing decisions being right —
and the hedge works even with noisy weights, so the LBR advantage
survives.

## The fix

Replace the 20 features with a set that is a **pure function of the
opponent's own actions**. The current EWM stats need to be recomputed
without hero-conditional gating:

- Replace "3bet when facing an open" with "raise frequency preflop
  across all hands".
- Replace "cbet when PFA" with "bet frequency on the flop across all
  hands" (already what `EWM_AGGRESSION` is doing).
- Drop the opportunity counters entirely (they are denominators, not
  features).
- Drop `trend_z` (session leak).

A ~10-feature set of unconditional opponent frequencies would:
1. Be trainable (fewer inputs, less overfitting, honest CV).
2. Transfer to deployment (no seat or hero-policy coupling).
3. Have a clear pass/fail on the archetype-classification task — either
   the archetypes are distinguishable from their raw frequencies or
   they're not, and if they're not, the mixture is dead on arrival and
   we should know that.

## Session-level result

Today did not improve the router. It replaced a vacuous 0.761 (on biased
data) with an honest 0.649 (on the correct data). That is progress — the
0.761 was a lie. The next session starts from 0.649 with a real list of
feature-engineering work.
