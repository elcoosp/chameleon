# Argmax fallback really matters: expert misses are common on out-of-family opponents (2026-09-30)

## The observed anomaly

Two bundles, identical except for the robust slot:

- `agent-honest`: 4×500k experts + 500k robust
- `agent-honest-5Mrobust`: 4×500k experts + **5M robust**

On `--agent full` (argmax+synthetic), 2500 deals/pair:

| opponent | 500k robust | 5M robust | Δ |
|---|---:|---:|---:|
| jamfix | +4 787 | +3 663 | **−1 124** |
| pnash  | +4 168 | +3 167 | **−1 001** |
| nit    | +1 384 | +1 885 | +501 |
| tag    | +3 382 | +3 713 | +331 |
| mean   | +7 136 | +6 983 | −153 |

The bundle is identical except for the robust policy, and the robust
policy is 10x better on LBR. Yet the argmax ladder gets **worse** with
the better fallback, driven entirely by jamfix and pnash.

## Why the fallback matters despite `fb_used = 0`

The `probe --diag-fallback` output for agent-honest with `--agent full`:

    jamfix    decis=80  fb_used=0  e0_miss=40 e1_miss=40 e2_miss=40 e3_miss=40  r_miss=0  reach0=40
    pnash     decis=106 fb_used=1  e0_miss=53 e1_miss=53 e2_miss=53 e3_miss=53  r_miss=1  reach0=53

**On 40 of 80 jamfix decisions, ALL FOUR experts miss their infoset.**
Same for pnash: 53 of 106. These are out-of-family opponents whose
game tree the archetype-trained experts never covered.

For those decisions, `argmax` falls through to `robust_sigma`:

    match expert_sigma[k].as_ref() {
        Some(s) => s.clone(),
        None => match robust_sigma.as_ref() {
            Some(s) => {
                tier_missed = true; // set, but...
                s.clone()
            }
            ...

The action IS taken from `robust_sigma`. So the fallback's quality
determines play on 50% of the decisions against jamfix and pnash.

But the `tier_missed` bit is then **reset**:

    if expert_missed[k] && robust_sigma.is_none() {
        tier_missed = true;
    } else if expert_missed[k] && robust_sigma.is_some() {
        tier_missed = false;  // <-- hides the case where robust covered
    }

So `fb_used` (which sums `tier_missed`) reports zero — the fallback
*was* used, but the metric doesn't count it.

## What this means

**The `fb_used` bit is not "was the robust used".** It is "did both
tiers miss". For a fair reading of the fallback's importance, a future
session should add a distinct counter `expert_missed_robust_covered`
that increments when `expert_missed[k] && robust_sigma.is_some()`.

The metric's current name is misleading, and the misleading name is
why the hybrid ladder result (which I initially read as "the fallback
doesn't matter on argmax") is wrong.

## The corrected finding

The 5M robust fallback is:
- **better** on nit (+501), tag (+331), famB (+267) — opponents where
  the picked expert rarely misses (its trained archetype is the same
  or similar)
- **worse** on jamfix (−1 124) and pnash (−1 001) — opponents where
  the expert almost always misses and the robust does the work

The 5M robust policy is better on LBR (against a uniform best
responder) but worse against the specific archetype pool on the
out-of-family lines. That is a real LBR-vs-ladder decoupling *inside
the fallback slot*.

## What to do

1. **Add a distinct fallback-metric.** `expert_missed_robust_covered`
   separate from `tier_missed`. The existing `fb_used` should keep its
   current meaning (double-miss only) for trace continuity.
2. **Re-bench the hybrid on the top-line mean.** Currently the swap is
   net −153. If the ship objective is mean ladder, the 500k robust
   fallback is preferred. If the objective is LBR, the 5M robust is
   preferred.
3. **Consider training the 5M robust against the archetype pool** in
   addition to LBR. The current `--mode robust` trains for robust
   best-response LBR, which is the wrong objective for the fallback
   role.

## Artifacts

- `artifacts/ladder-hybrid-5Mrobust-full.log`
- `artifacts/ladder-agent-full-honest-full.log`
- The probe diag output: run `chameleon probe --diag-fallback --agent full --bundle artifacts/agent-honest`

## Related

- `HYBRID-LADDER-2026-09-30.md`
- `LBR-VS-LADDER-2026-09-30.md`
