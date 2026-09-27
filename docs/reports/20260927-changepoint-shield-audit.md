# ChangepointShield audit (v7 Item 6, 2026-09-27)

## What was checked

`crates/cham-router/src/runtime.rs::ChangepointShield`. The doc-comment
calls it "Bayesian online changepoint shield (Adams & MacKay 2007-style
run-length posterior)". Audit: run the detector on constant evidence for
50 hands and observe whether the run-length posterior evolves.

## Finding: the recursion is inert

With constant per-archetype evidence `[0, -3, -3, -3]` for 50 hands:

    hand 10: p_recent=0.0248 eff_n0=7.8020
    hand 20: p_recent=0.0248 eff_n0=7.8020
    hand 30: p_recent=0.0248 eff_n0=7.8020
    hand 40: p_recent=0.0248 eff_n0=7.8020
    hand 50: p_recent=0.0248 eff_n0=7.8020

p_recent and eff_n0 are FROZEN from hand 10 onward. The recursion

    let best = lik.iter().copied().fold(0.0, f64::max).max(1e-9);
    next[0]   = h * total * best;
    next[r+1] = (1 - h) * posterior[r] * best;

uses a single scalar `best` for every run length `r`. Since the growth
likelihood does not depend on `r`, the recursion is a scalar multiple
of the previous posterior plus a constant reset term - a smoothing
operation, not a Bayes update. The ONLY thing that changes the
posterior shape is the separate vote-change branch:

    if v != lv {
        next[0] += (posterior[5..].sum()) * 0.5;
        for r in 5..n { next[r] *= 0.5; }
    }

## What this means

`ChangepointShield` is, in effect, a vote-change detector with a
posterior-shaped accumulator. It is not an Adams & MacKay detector:
that algorithm requires per-run predictive likelihoods (a different
likelihood for each run length, from a per-run posterior over the
archetype distribution). The doc-comment overstates the implementation.

## Severity: low

The flag is opt-in (`CHAM_ROUTER_CHANGEPOINT=1` or
`--router-changepoint-shield`). It does not corrupt training, does not
affect any default, and does not touch the blueprint. The two tests in
`crates/cham-router/tests/router.rs` pass, but they only assert
direction (`eff > 4.0` stationary; `after < before` switching) and so
did not catch the inertness.

## Recommendation

Rename to `VoteChangeShield` (4 call sites: `runtime.rs`, `lib.rs`,
`hero.rs`, `self_exploit.rs`), fix the doc-comment to say what the
shield actually is, and add a test that pins the observed frozen
behavior on constant evidence. Optional: implement a real Adams &
MacKay detector with per-run posteriors, but the vote-change signal is
the actual defense mechanism and does not need the extra state.

Severity: cosmetic + documentation. Not fixed in this commit; the
call is the repo owner's since the flag is not on the critical path.
