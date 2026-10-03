# Lever comparison: tree richness vs bucket fineness (2026-10-03)

## Corrected BR, both seats, 5000/500/30

| policy | seat0 tab | seat1 tab | sum | note |
|---|---:|---:|---:|---|
| tiny robust (current trainer) | -2.03 | +0.58 | **-1.29** | baseline |
| **rich-lite robust** (2 sizes/street, cap 1, slot bucket) | -1.72 | +1.33 | **-0.40** | tree richness |
| medium 64/32/32 (Sep-29 artifact) | +10.70 | +11.63 | **+22.34** | OLD TRAINER |

## Tree richness: small positive

rich-lite sums to **-0.40** vs tiny's **-1.29** — less exploitable.
Notable because rich-lite has 190k infosets (vs tiny 80k), so its
tabular-BR learner is *harder* to converge; a less-converged learner
tends to look MORE exploitable. It looks less. So the direction is
trustworthy and the true improvement may exceed the 0.9 bb shown.

Both are within the learner's residual gap (~1 bb), so this is a
"small positive", not a decisive win. But it is the first corrected
evidence that adding a second bet size helps.

## Bucket fineness: UNTESTED (medium is a stale artifact)

The medium measurement (+22.34) is **uninformative about buckets**: the
artifact is from 2026-09-29, before F3/F4/F6a. It is an old-trainer
policy, so +22 bb reflects the trainer gap (like the old shipped bundle
at +15.4), not the effect of 64/32/32 buckets.

To test buckets, medium must be **retrained with the current trainer**
(same as rich-lite). Cost ~1.5h (medium has ~160k post-F6a infosets,
similar to rich-lite's 190k).

## The lever map (updated)

| lever | status | verdict |
|---|---|---|
| tree richness (rich-lite) | measured | **small positive** (-0.40) |
| bucket fineness (medium) | NOT measured | needs a current-trainer retrain |
| DCFR schedule (γ=2) | measured | positive (-0.70 vs -1.45) |
| search (F10) | not wired | pending |
| opponent modelling / ladder | untouched | pending |

## Recommendation

1. **Keep rich-lite** — it is the current best corrected-metric policy
   and trains in ~1.5h.
2. **Retrain medium with the current trainer** as the bucket test —
   ~1.5h, the same cost as rich-lite. If medium-current beats rich-lite,
   buckets are the stronger lever; if not, tree richness is.
3. Do **not** trust the +22 medium number for any decision.

Source: `artifacts/richlite-robust-early-2026-10-03/`,
`artifacts/medium-corrected-br-2026-10-03/`.
