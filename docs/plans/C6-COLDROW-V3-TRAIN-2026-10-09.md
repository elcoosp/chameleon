# C-6 cold-row discards during v3 training (2026-10-09)

The plan's C-6 finding: `Traversal` returns `NaN` for a missing row in
the parallel phase and every ancestor skips its update — a selection
bias, telemetered as `cold_rows`.

The v3 training run (`real-full-v3`, 15M iters, real-full buckets)
reports the discard rate per slice:

    slice 1875000:  7.507% of hero nodes (1073939 / 14306361)
    slice 3750000:  3.034% (392042 / 12920123)
    slice 5625000:  1.553% (222368 / 14319124)
    slice 7500000:  0.937% (143043 / 15271323)

**The discard rate decays with iterations** (7.5% -> 0.9% by 7.5M), which
is the expected shape: cold rows are infosets not yet visited; as the
table fills, fewer nodes are cold.

## Why this matters

1. **Early-training bias is measurable and now quantified.** In the
   first 1.9M iterations, 7.5% of hero-node updates are skipped. A
   trainer that reports "5M iterations" of updates actually applied
   fewer at the unvisited frontier.
2. **The bias is not constant.** It falls as the table fills, so its
   effect is front-loaded — the early iterations (which carry the most
   weight in DCFR's t-schedule) are the most affected.
3. **This is what the plan's C-6 fix targets.** A "warmup slice"
   (single-threaded insert pass before parallel phases) is the standard
   cure; the shipped trainer has one (`warmup=50000`). The question is
   whether 50k warmup iterations are enough — the v3 telemetry says
   7.5% is still cold *after* the warmup, at slice 1.875M.

## The suggestion for W2 / the trainer

Raise the warmup, or run the first slice single-threaded until the
cold-row rate falls below a threshold (say 1%). The current warmup is
a fixed 50k; the data says the needed warmup scales with the infoset
count (v3 has 15M+ hero-node visits vs v2's fewer).

Not urgent — it is a quality knob, not a correctness bug. But the v3
telemetry makes the right value measurable instead of guessed.
