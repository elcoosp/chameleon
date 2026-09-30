# Hybrid bundle: 5M robust fallback does not improve the mixture ladder (2026-09-30)

## Setup

`artifacts/agent-honest-5Mrobust` is `artifacts/agent-honest` with the
robust policy slot replaced by the par-5M robust policy. Same 4 experts,
same router, same abstraction, same buckets. Only the fallback differs:

| slot | agent-honest | agent-honest-5Mrobust |
|---|---|---|
| experts (×4)   | 500k each | 500k each (same files) |
| router.bin     | same | same |
| robust         | 500k robust | 5M robust |

Verified by SHA-256: experts match, robust differs by hash.

## Results (ladder --fast, 2500 deals/pair)

| opponent | agent-honest (500k fallback, 09-28) | agent-honest-5Mrobust (5M fallback) | Δ |
|---|---:|---:|---:|
| arch:nit      | +1 593 | +1 592 | −1 |
| arch:tag      | +3 185 | +3 289 | +104 |
| arch:lag      | +5 567 | **+5 618** | +51 |
| arch:station  | +12 746 | +12 788 | +42 |
| callbot       | **+11 170** | **+11 170** | **0** |
| jamfix        | −387 | −387 | 0 |
| pnash         | +222 | +261 | +39 |
| famB:tag      | +635 | +575 | −60 |
| noisy:0.1:lag | +4 770 | +4 708 | −62 |
| **mean**      | **+4 388** | **+4 401** | **+13** |

The 09-28 mixture numbers are quoted from `SOTA-2026-09-28.md`
(synthetic-router column). The 5M-fallback numbers are from
`artifacts/ladder-hybrid-5Mrobust-full-mixture.log`.

## The finding

**Swapping the robust fallback from 500k to 5M changes the mixture
ladder by +13 mb/seating — well inside run-to-run noise.** The
mixture's decision path doesn't rely on the robust slot enough for
the fallback's quality to matter.

The mechanism: mixture routing blends all 5 policies with reach-weighted
weights. The 4 experts dominate. The robust slot has weight
`w[4] = 1 − Σ w[k]`, which is small when the router is confident and
only becomes significant when the shield fires (trend_z < shield_z, a
rare condition). So the fallback is almost never load-bearing in
mixture mode.

This is **not** true for `robust-only` mode, where the robust policy
is the only one playing. There, the fallback's quality matters:
`agent-honest`'s robust-only ladder was +720; the 5M robust-only
ladder was +820. Difference of +100, still small but non-zero.

This is also **not** true for `hedged` mode (which is broken anyway,
see `HEDGED-PATH-BUG-2026-09-30.md`).

## The `full` (argmax) row is missing

`artifacts/ladder-hybrid-5Mrobust-full.log` is only 57 bytes (header
only) because the first mode's log was truncated when I killed a
duplicate pipeline invocation at 12:11. **It needs re-running** — the
script is on disk and safe (has the concurrency lock).

Expected: `full` (argmax+synthetic) should give +6587 like
`agent-honest`. The robust slot isn't on the argmax path directly
(argmax picks among the 4 experts), but the `robust_sigma` fallback
fires when the picked expert's infoset is missed. So there could be
a small difference. Re-running will tell.

## What this means for the frontier

**The LBR-improving work (delay0+eps02, avguniform, etc.) targets the
robust policy, but the robust policy barely affects the shipped
mixture ladder.** The 5M-vs-500k upgrade on the robust slot is
worth ~+100 on robust-only and ~+13 on mixture. If the delay0+eps02
robust policy is another +100 on robust-only, it will be
**undetectable** on the mixture ladder.

**The mixing router is the SOTA, and the SOTA is dominated by the 4
experts + router**, not by the robust fallback. To improve the
shipping ladder, the next steps are:

1. Retrain the 4 experts at higher budgets (e.g. 5M each). LBR isn't
   the target — the ladder is.
2. Improve the router's feature set (TAG/LAG separation).
3. Investigate why `callbot` gets +24 962 on the argmax measurement
   but only +11 170 on the mixture. That is a routing effect and the
   single largest number in the matrix.

## Artifacts

- `artifacts/agent-honest-5Mrobust/`  (hybrid bundle, sha-verified)
- `artifacts/ladder-hybrid-5Mrobust-full-mixture.log`
- `artifacts/ladder-hybrid-5Mrobust-robust-only.log`
- `artifacts/ladder-hybrid-5Mrobust-full.log` — **TO BE RE-RUN**

## Related

- `LBR-VS-LADDER-2026-09-30.md` — the LBR/ladder decoupling
- `PAR5M-ROBUST-LADDER-2026-09-30.md` — the robust-only ladder
- `LADDER-ARMMAX-REPRODUCED-2026-09-30.md` — the argmax reference
