# The γ-underflow bug: 40 % LBR win for one config change (2026-09-28)

## The number

Tiny abstraction, 500k iters, seed 7, depth 100, 200 deals:

| averaging scheme | seat 0 | seat 1 |
|---|---:|---:|
| uniform (CHAM_AVG_UNIFORM=1)         | 25 651 | 15 184 |
| **γ = 1.0 (Linear CFR+, new default)** | **23 280** | **13 957** |
| γ = 0.9 (OLD DEFAULT)                | 39 692 | 15 068 |
| uniform (no trained weights)          | 48 186 | 28 013 |

**The old default (γ = 0.9) was 70 % worse on seat 0 than Linear CFR+**
(39 692 vs 23 280) and 8 % worse on seat 1.  The fix is one line in two
places.

## The bug

The averaging weight formula was

    w_t = (t − T/4) · γ^(T−t)         (robust mode)

`γ^(T−t)` with γ = 0.9 underflows to **exactly zero in f64** for
`T − t ≳ 700` (0.9^700 ≈ 10⁻³²; 0.9^1000 ≈ 10⁻⁴⁶; 0.9^3000 ≈ 10⁻¹³⁸).
The effective averaging window therefore collapses to the last ~700
iterations regardless of whether the run is 500k or 5M.  Since CFR+'s
CURRENT iterate oscillates for a long time before converging, averaging
only the tail of that oscillation produces a much worse strategy than
averaging from the delay point on.

`default_avg_gamma()` in the library was changed to `1.0` (Linear CFR+),
and the CLI's clap `--avg-gamma` default was changed from `0.9` to `1.0`
in both `main.rs` and `cmd/self_exploit.rs`.  A runtime tripwire now
warns when a user's chosen γ underflows within their run.

## Why this was invisible

1. The 50k / 500k / 5M sweep looked flat (44 → 40 → 37), which was
   attributed to "abstraction-limited".  In fact all three were
   averaging the last ~700 iters of a 500k-iter-oscillating policy.
2. `default_avg_gamma()` in the library WAS changed first — but the CLI
   had its own hardcoded `0.9` in clap, and self_exploit had a third
   hardcoded `0.9`.  The library's `#[serde(default)]` only applies to
   deserialization; the CLI constructs the struct directly.
3. The existing `delayed_averaging_monotone` test used `total = 2000`, at
   which `0.9^2000 ≈ 10⁻⁹²` — still underflow, but the test only checked
   that avg has "lower variance than current", which underflow satisfies
   trivially.

## Follow-ups

- The 50M run was killed and relaunched with the new default
  (`artifacts/blueprints-retrain-tiny-50M/`, log
  `artifacts/retrain-tiny-50M-v2.log`).  It answers "does the trainer
  still benefit from 100× more iterations once averaging is fixed?"
- The old 50M partial (γ = 0.9) is preserved as
  `artifacts/blueprints-retrain-tiny-50M-OLD-GAMMA/` for reference.
- The averaging DELAY `D = T/4` is now the next thing to review: it
  throws away the first 25 % of iterations from the strategy sum.  A
  quick A/B against `D = 0` (pure `w_t = t`, the standard Linear CFR+
  weight) will tell us whether the delay helps or hurts at this
  abstraction.
