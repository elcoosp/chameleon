# Instrument seat asymmetry is why 45% of the router features are dead (2026-09-29)

## The smoking gun

My earlier diagnostic found that 9 of 20 router features are constant
across the 120 000-row instrumented dataset:

- 3 opportunity counters never leave 0 (`opp_faces_open`,
  `opp_faces_3bet`, `opp_cbet_opportunities`)
- 6 EWM stats never leave their 0.5 initial value (`3bet`,
  `fold_to_3bet`, `call_3bet`, `cbet_flop`, `fold_to_cbet`, `limp`)

## Why

The instrument tool (`cmd/collect.rs` in the real path; previously
`/tmp/instrument/`) runs the SHIPPED hero at **seat 0** (SB) for every
hand. In heads-up, **SB acts first preflop**. So preflop:

1. Hero is always first to act.
2. Opponent (BB) acts second.
3. `facing_open` fires only when hero LIMPS (or calls) and BB then
   raises — a rare line.
4. `facing_3bet` fires only when hero opens, BB 3bets, and hero faces
   a decision — common enough but produces `EWM_FOLD_TO_3BET` and
   `EWM_CALL_3BET` only.
5. `EWM_LIMP` fires only when hero limped and BB called — but BB
   checks in that line, so it never actually fires.
6. `EWM_PFR` fires only when hero limped and BB raised — rare.
7. `opp_is_pfa = pfr || opp_3bet` — most flop cbet opportunities never
   register because the opponent is rarely the preflop aggressor.

## The H-2 parallel

This is structurally the same bug as H-2 (`tracker models the wrong
opponent in every seat-1 seating`). H-2 was fixed at the pipeline level
by recording the hero's actual seat. But the same problem exists at the
**data-collection** level: if the instrument always runs hero=SB, the
tracker sees a biased slice of opponent behavior — the slice in which
the opponent acts second preflop.

The 500k-tiny retrain's `probe --diag-fallback` output confirms the
counter behavior is fine at the pipeline level (fallback rate 0.6 %),
so the tracker is working; the dataset is what's biased.

## What to fix

`cmd/collect.rs::run_real` must alternate the hero seat across sessions:

    for session in 0..sessions {
        let hero_seat = (session % 2) as usize;   // alternate SB/BB
        // then build hero, run hand with hero at that seat
    }

The `play_one_hand` helper must accept a hero_seat parameter and:
- view observables from `hero_seat`
- apply `on_public_action` with the actor's seat
- feed `on_hand_end` with the correct net

This mirrors what `MatchRunner` already does for training data. Once
hero alternates, both SB and BB perspectives get recorded; the tracker
sees the opponent's full preflop action menu; and the dead features
come alive.

## Impact

9 of 20 features are currently constant. Fixing seat alternation should
turn on most of them. The router's top-1 (0.761) should climb
substantially, and the gate (0.80) may become reachable without any
feature-set changes.

The mixture might then beat argmax on the ladder. That is a meaningful
shift in the SOTA config.
