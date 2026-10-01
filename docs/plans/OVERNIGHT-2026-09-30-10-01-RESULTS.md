# Overnight results (2026-09-30 22:00 → 2026-10-01 01:10)

## 5M expert retrain: NEGATIVE

The retrain (using the new parallel exploit path) produced all four
experts successfully:

| expert | 500k size | 5M size |
|---|---:|---:|
| nit | 368 KB | 454 KB |
| tag | 393 KB | 410 KB |
| lag | 398 KB | 413 KB |
| station | 432 KB | 444 KB |

Ladder `--fast`, 2500 deals/pair:

| routing | 500k experts (19dim router) | 5M experts (synthetic router) | Δ |
|---|---:|---:|---:|
| `full` (argmax) | **+8 276** | +6 425 | **−1 851** |
| `full-mixture` | **+6 078** | +5 586 | −492 |

**The 5M expert retrain makes the ladder worse, not better.** Even
before accounting for the router difference, the 5M experts lose
their per-opponent sharpness on `arch:lag`, `callbot`, and
`noisy` compared to the 500k ones.

**Two confounds:** (1) this bundle uses the *synthetic* degenerate
router, so the argmax result is "one 5M expert" not a routed
mixture. (2) The 500k experts were the ones that the 19-dim router
was trained against — the router may not transfer to 5M experts.

**Net:** the 5M expert path is a dead end unless combined with
retraining the router on 5M-expert data. Not worth the ~5h compute.

## Router sharpening-temperature sweep: NON-LEVER

Three runs on the 19-dim SOTA bundle at temp = 0.5 / 0.7 / 1.0:

| temp | mean |
|---|---:|
| 0.5 | +8 236 |
| 0.7 (default) | +8 259 |
| 1.0 | +8 210 |

**All within ±25 mb/seating.** The calibration is already folded
into the model weights, so the runtime sharpening has negligible
effect. The temp field is essentially a no-op for this bundle.
(Documentation gotcha for future sessions.)

## The 19-dim SOTA is final for today

`artifacts/agent-honest-19dim` with `--agent full` at **+8 276** remains
the best ladder mean the project has measured. No overnight run beat it.

## Landed fixes overnight

- F4 (`2d5c3d8`): parallel Robust warmup now runs real CFR+ updates
  (was: insert-only, wasting 20% of every run).
- F6 (`2d5c3d8`): `RegretTable::iter()` yields `(key, off, w)`;
  removes three O(n²) scans in snapshot/artifact/warm-start.
- F5 (`562c41e`): parallel checkpoint runs the renorm pass before
  snapshot (f32 guard now applies to parallel runs).

## Still in progress

F1, F2, F3, F7, F8, F9, F10 — see `COMPETITIVE-REVIEW-2026-10-01.md`.

## Artifacts

- `artifacts/ladder-5M-experts-{full,mixture}.log`
- `artifacts/ladder-19dim-full-temp{0.5,0.7,1.0}.log`
- `artifacts/agent-honest-5M-experts/` (do not ship)
- `artifacts/agent-honest-19dim/` (current SOTA)
