# Matched-visits tree effect: the 2-size tree is ~neutral (2026-10-05)

Full-coverage buckets, slot bucket, DCFR g2, 15k-deal budget (bound-gated).

| arm | visits/infoset | seat 0 | note |
|---|---:|---:|---|
| tiny-full @5M | 60 | +4.13 | 1 size, 83k infosets |
| rich-lite-full @5M | 26 | +5.40 | 2 sizes, 191k infosets |
| rich-lite-full @12M | 60 | **+4.82 +/- 0.68** | matched visits |

## Finding: the "more sizes = worse" result was a visits artifact

At UNMATCHED visits (both @5M), rich-lite looked worse (5.40 vs 4.13).
At MATCHED visits (rich-lite @12M = tiny @5M = 60 visits/infoset), the
gap collapses to 4.82 vs 4.13 = **0.69 bb, inside the ~1 bb combined
SE**. The two-seat sum SE on such a difference is ~1.4 bb, so this is
NOT significant.

**Conclusion: at matched training, adding a second bet size is ~neutral**
on within-abstraction exploitability. Neither a clear win nor a clear
loss. The earlier apparent regression was under-training.

## Caveats

- **Seat 1 is missing**: the measurement was SIGTERM'd by an external
  process at 45 min, before seat 1 printed. Seat 0 is the convergence
  signal and suffices for the direction; a clean seat-1 point would
  tighten it.
- Within-abstraction BR is not a clean cross-abstraction comparison
  (a richer abstraction admits more exploitation by construction).
- Single seed.

## Recurring external SIGTERM

Two long nextest runs were killed mid-flight by an external SIGTERM
(this one at 2708s; a 30000-deal run earlier at 13636s). No panic, no
OOM, no reboot. Something outside the process tree is terminating long
runs. Until that is understood, both-seat measurements over ~40 min are
unreliable here. Mitigation: run seats as SEPARATE short processes, or
find the killer.

## Where the levers stand (after this)

| lever | verdict |
|---|---|
| DCFR vs CFR+ | neutral (10.27 vs 10.11) |
| 2 bet sizes | ~neutral at matched visits (4.82 vs 4.13, NS) |
| bucket coverage | real correctness fix (removed suit-blind fallback); ~8-10 bb exploitable revealed |
| search | broken upstream (map_to_legal) — fix landed, needs A/B |
| real-game gap | unmeasured (no external anchor) |

No abstraction tweak is a demonstrated lever. The remaining first-order
work is **search** and **the real-game gap**.
