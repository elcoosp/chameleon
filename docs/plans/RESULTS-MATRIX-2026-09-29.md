# Results matrix — every measured config (updated 2026-09-29 evening)

LBR at depth 100. Lower is better. Sample sizes noted per row.

## Tiny abstraction (~21k infosets)

### No exploration floor

| config | deals | seat 0 | seat 1 | mean |
|---|---:|---:|---:|---:|
| serial 500k | 200 | 23 280 | 13 957 | 18 619 |
| parallel 5M | 1000 | **13 977** | **12 858** | **13 417** |
| parallel 5M | 200 | 15 040 | 12 050 | 13 545 |
| parallel 20M | 1000 | **13 319** | 14 706 | 14 012 |
| parallel 20M | 200 | 13 809 | 14 956 | 14 383 |
| parallel 50M | 1000 | 13 572 | 16 346 | 14 959 |
| parallel 50M | 200 | 13 886 | 17 008 | 15 447 |
| parallel 20M delay0 (CHAM_AVG_DELAY=0) | 1000 | 13 431 | 13 618 | 13 524 |
| parallel 20M avguniform (CHAM_AVG_UNIFORM=1) | 1000 | 13 977 | 13 133 | 13 555 |
| parallel 20M delay0+eps=0.02 | 1000 | 13 608 | 13 251 | 13 429 |

The two 20M schedule ablations (`delay0`, `avguniform`) both move BB by
7–11% toward the 5M peak, at a small SB cost. Mean is a wash between
them. See `AVG-DELAY-DELAY0-RESULT-2026-09-29.md` and
`AVG-UNIFORM-RESULT-2026-09-29.md`.

### With the insert-only warmup fix (1fa3762) — no floor

| config | deals | seat 0 | seat 1 | mean |
|---|---:|---:|---:|---:|
| parallel 20M warmfix | 1000 | 13 554 | 14 090 | 13 822 |
| parallel 5M warmfix  | 1000 | 14 648 | 12 674 | 13 661 |

Warmfix helps BB, hurts SB. At 5M the SB cost dominates and warmfix is
a net loss; at 20M the BB recovery dominates and warmfix is a net win.
See `WARMUP-FIX-RESULT-2026-09-29.md` (20M) and
`WARMUP-FIX-AT-5M-2026-09-29.md` (5M).

### With an exploration floor (eps = 0.02)

| config | deals | seat 0 | seat 1 | mean |
|---|---:|---:|---:|---:|
| parallel 5M eps=0.02 | 1000 | 14 910 | 12 551 | 13 731 |
| parallel 20M eps=0.02 | 1000 | 13 682 | 13 652 | 13 667 |

### With regret discount (DCFR)

| config | deals | seat 0 | seat 1 | mean |
|---|---:|---:|---:|---:|
| parallel 20M alpha=0.9 | 1000 | 35 344 | 25 834 | 30 589 |
| parallel 20M alpha=0.5 | 1000 | 38 721 | 29 102 | 33 912 |

Both alpha=0.9 and alpha=0.5 un-freeze the iterate — the freeze
diagnostics improve as the discount shrinks (avg_near_frozen
60% → 11.9% → 9.7%, mean avg max_p 0.859 → 0.616 → 0.579) — but
the LBR collapses monotonically (mean 14 012 → 30 589 → 33 912).
Un-freezing to random is far worse than freezing to a decent
approximate equilibrium. See `DCFR-ALPHA09-NEGATIVE-2026-09-29.md`
and `DCFR-ALPHA05-NEGATIVE-2026-09-30.md`.

**The regret-discount lever is dead at any discount <= 0.9.**

## Full abstraction (~380k infosets)

| config | deals | seat 0 | seat 1 | mean |
|---|---:|---:|---:|---:|
| parallel 9M | 200 | 18 079 | 14 298 | 16 188 |

Matched-visits comparison (tiny-500k and full-9M both ~24 visits/infoset):
- SB: full wins by 22%
- BB: tiny wins by 2%

## Medium abstraction (~84k infosets)

| config | deals | seat 0 | seat 1 | mean |
|---|---:|---:|---:|---:|
| parallel 20M | 1000 | 13 237 | 14 021 | 13 629 |

New SB SOTA at the 20M budget, but BB still collapsed. See
`MEDIUM-20M-LBR-2026-09-29.md`.

## Baselines vs the archetype pool (ladder --fast, 2500 deals/pair)

Original 09-28 measurement (agent-full-honest, per `assemble-full.log`):

| agent | mean mb/seating | wins |
|---|---:|---:|
| uniform | ~0 | — |
| full-mixture (synthetic router) | +4 388 | 6/9 |
| full (argmax) | **+7 146** | **9/9** |
| full-hedged | −1 994 | 0/9 |

2026-09-30 re-measurements, all on tiny abstraction, 2500 deals/pair:

| bundle / routing | mean | wins |
|---|---:|---:|
| **agent-honest / full (argmax+synthetic)** | **+7 136** | 9/9 |
| agent-honest-5Mrobust / full (argmax+synthetic) | +6 983 | — |
| agent-honest / full-mixture | +4 388 | 6/9 |
| agent-honest-5Mrobust / full-mixture | +4 401 | — |
| agent-honest / robust-only | +720 | 3/9 |
| par-5M robust-only | +820 | 3/9 |
| agent-honest / full-hedged | **+7 107** | 9/9 (≈ argmax; pre-fix runs measured CallBot) |

**Correction (13:20):** the `SOTA-2026-09-28.md` doc's stated argmax
mean (+6 567) does not match its own per-opponent column (sum 64 085
→ mean 7 120). The 2026-09-30 re-measurement inherited the same error.
The true argmax mean for `agent-honest` is **+7 136**.

**Key findings:**
- The **robust-only policy alone is far weaker than argmax over 4
  experts** (~6 000 mb/seating gap). LBR SOTA (par-5M robust) is not
  ladder SOTA. See `PAR5M-ROBUST-LADDER-2026-09-30.md`.
- **Upgrading the robust fallback slot from 500k to 5M helps argmax
  by +402 mb/seating but does not move the mixture.** See
  `HYBRID-LADDER-2026-09-30.md`.
- **Hedged routing is broken** — even at threshold 0.00 it diverges
  from argmax by 8 400 mb/seating. See
  `HEDGED-PATH-BUG-2026-09-30.md` and
  `HEDGED-SWEEP-CONFIRMS-PATH-BUG-2026-09-30.md`.

## The frontier

**Best LBR seat 0 at 1000 deals:** tiny 20M no-fix, 13 319.
**Best LBR seat 1 at 1000 deals:** tiny 5M no-fix, 12 858.
**Best LBR mean at 1000 deals:** a tie — tiny 5M no-fix (13 417)
and tiny 20M delay0+eps02 (13 429), within 12 mb/hand of each other.

The **shipping candidate** is still the tiny 5M no-fix robust policy:
mean 13 417, 47 min serial / ~10 min parallel, minimal configuration.

The **alternate shipping candidate** is tiny 20M delay0+eps02: mean
13 429 (statistical tie), SB 13 608 (better than 5M's 13 977), BB
13 251 (worse than 5M's 12 858), ~44 min parallel. Better SB, worse
BB. Choose per ladder preference.

**The single most effective lever this session is the averaging
schedule.** Both delay0 (13 524) and avguniform (13 555) cut 20M BB
by 7-11% from the 14 706 baseline; delay0+eps02 cuts it further to
13 251. No lever alone closes the gap to 5M's 12 858; the schedule
plus floor matches the 5M mean but shifts the seat balance.

## What didn't win

- **Exploration floor (eps=0.02)** — helps at 20M (mean −2.5%), hurts
  at 5M (mean +2.4%). Slower collapse, no reversal. See
  `EXPLORATION-FLOOR-AT-20M-2026-09-29.md` and
  `EXPLORATION-FLOOR-NEGATIVE-2026-09-29.md`.

- **Insert-only warmup fix (1fa3762)** — 4% BB improvement at 20M;
  +671 SB / −184 BB at 5M (net loss). See
  `WARMUP-FIX-RESULT-2026-09-29.md` and `WARMUP-FIX-AT-5M-2026-09-29.md`.

- **DCFR regret discount alpha=0.9** — un-freezes the iterate but
  destroys the policy. 2.5x worse than no-fix on both seats. See
  `DCFR-ALPHA09-NEGATIVE-2026-09-29.md`.

- **Full abstraction at 9M** — beats tiny-500k at matched
  visits/infoset, loses to tiny-5M on wall.

- **Medium abstraction at 20M** — new SB SOTA at 20M, but inherits the
  RM+ freeze on BB. See `MEDIUM-20M-LBR-2026-09-29.md`.

- **Mixture routing, hedged routing** — measured worse than plain argmax
  against the archetype pool.
