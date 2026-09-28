# The router feature set is hero-dependent (2026-09-29)

## The realization

Chasing the 9 dead features further into the tracker's logic revealed
something more fundamental than seat asymmetry. Most of the "opportunity-
gated" EWM stats fire only under specific HERO behaviours. Against a
passive hero, they never fire at all.

## The gating conditions, read literally

The tracker updates each stat only if the corresponding opportunity
fires. Those opportunities are hero-conditional:

| feature | gating condition | requires hero to |
|---|---|---|
| EWM_3BET | `facing_open` | open preflop |
| EWM_FOLD_TO_3BET | `facing_3bet` | open AND THEN 3bet |
| EWM_CALL_3BET | `facing_3bet` | (same) |
| EWM_CBET_FLOP | `opp_is_pfa && reached_flop` | limp so opponent becomes PFA |
| EWM_FOLD_TO_CBET | `opp_bet_faced && street >= 1` | bet postflop first |
| EWM_LIMP | `is_opp && raises_this_street == 0` after Call | check preflop |

Look at `facing_open`: it fires when `is_opp == false && raises_this_street == 0`,
i.e. when **the hero opens**. So `EWM_3BET` only updates on hands where
the HERO opened. If hero never opens, this EWM is never updated and
stays at its 0.5 initial value.

Look at `opp_is_pfa = pfr || opp_3bet`. `pfr` fires when the OPPONENT
raises first preflop. So for the opponent to be preflop aggressor, hero
must **not** raise preflop — hero must limp or check (from BB). If the
shipped agent is a passive limper (very likely at 500k iters on tiny),
`opp_is_pfa` rarely fires and `EWM_CBET_FLOP` never updates.

## The consequence

**The same opponent will produce different router features against
different hero policies.** Against a passive hero, an aggressive opponent
looks identical to a passive opponent (because the opportunity counters
are silent). Against an aggressive hero, the same opponent produces a
rich feature vector.

This is a **leak** in the same category as `trend_z`:

- `trend_z` correlates with the opponent only because the instrument
  runs one session against one opponent. In deployment, it would
  correlate with the recent hand-history against *any* opponent.
- The opportunity-gated EWMs correlate with the opponent only because
  the hero's policy is fixed during collection. In deployment, the
  router would see hero-dependent features, not opponent-only features.

## What this means for the mixture architecture

The mixture's router is *supposed* to be an opponent model. Its input
features must describe the OPPONENT, not the (opponent, hero) pair. Two
of its 20 features are demonstrably not opponent-only (`trend_z` and
everything gated on hero aggression), and that's the reason the mixture
loses to argmax.

Argmax doesn't have this problem: it picks the max-weight expert from
whatever weights the router produces, and even a bad router produces a
single expert's policy that isn't worse than a random policy. The
mixture *averages* those bad weights into a policy that is worse than
the best single expert.

## What to fix

The features that are hero-conditional need to be redefined in terms
of **unconditional opponent tendencies**:

1. **VPIP, PFR**: already unconditional (opponent's own actions,
   independent of hero). Keep.
2. **3bet, fold-to-3bet, call-3bet, cbet, fold-to-cbet, limp**:
   redefine as raw action frequencies per street, not as
   opportunity-gated conditionals. E.g. `pfr / hands_with_flop` is a
   raw opponent frequency that doesn't require hero to open.
3. **Opportunity counters** (14-16): drop from the feature vector.
   They're denominators, not features. If we want opportunity-awareness,
   gate the *normalization* on them, not the value.
4. **trend_z**: drop. It's a session leak.

A smaller, unconditional feature set (say, 10 opponent-only
frequencies) would be honest about what the router is allowed to
know and would be trained on the same distribution it sees at
deployment.

## The bigger picture

This is the kind of leak the workspace's invariants were designed to
catch. I9 says "public history only" and H-5 fixed a real per-hand
gating bug. But the *feature engineering* layer above the tracker
was never audited for "does this feature describe the opponent or
the (opponent, hero) pair?"

That audit is now done. The answer is: 4 opponent-only, 4 hero-dependent,
3 dead (opportunity counters), 1 session leak (trend_z), 1 hands-since-
showdown (which is opponent-only-ish), and the rest EWM stats that are
opponent-only in principle but opportunity-gated in practice.

## Recommended next move

Do not try to fix the current 20-dim feature set. Replace it. A 10-dim
unconditional opponent-frequency feature set, trained against the same
real instrumented data, would give the router a fair chance. That's the
next session's work.
