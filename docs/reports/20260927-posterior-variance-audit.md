# posterior_variance() audit — computed but never consumed (2026-09-27)

## The claim

`crates/cham-router/src/runtime.rs:308-312`:

    /// `max(posterior_variance())`: high variance = the session hasn't
    /// identified the villain yet = route conservatively (robust-leaning).
    pub fn posterior_variance(&self) -> [f64; N_EXPERTS - 1] {
        self.last_post_var
    }

And at line 273, `last_post_var[k]` is populated as the Dirichlet marginal
variance on every `weights_for_hand` call.

## The reality

`posterior_variance()` has exactly **two callers**, both in
`crates/cham-router/tests/router.rs` (lines 272, 281) — neither reads the
value into anything that affects a decision. No code path in
`cham-agent`, `cham-cli`, or the router itself consumes the variance to
lean robust.

Grep of the whole workspace for a caller:

    crates/cham-router/src/runtime.rs:55  (definition, field)
    crates/cham-router/src/runtime.rs:196 (init to zeros)
    crates/cham-router/src/runtime.rs:273 (populate)
    crates/cham-router/src/runtime.rs:310 (accessor)
    crates/cham-router/src/runtime.rs:317 (reset)
    crates/cham-router/tests/router.rs:272, 281 (tests)

No other file references it.

## Terminology note (correction)

The field's doc-comment references "B2" as if B2 were a single named
gate. It is not. `docs/plans/v3-execution-roadmap.md:561` describes B2 as
**"router maturity"** — a research/measurement track that was never
broken down into individual gate items. There is no "B2 confidence gate"
elsewhere in SPECS or the plans; the doc-comment's phrase "the B2
confidence gate input" is itself a small inconsistency. The variance
value was computed on speculation that a gate would be added; it never
was.

## Why this matters

This is the **third instance this session** of "doc-comment claims a
gate that does not exist" — the same pattern as:

1. **RBP-gate bug** (`crates/cham-blueprint/src/traversal.rs`) — the
   doc said "pruning disabled at theta0=0"; the code pruned
   unconditionally. **DANGEROUS** — invalidated every pre-`fe84467`
   policy.
2. **ChangepointShield** (`crates/cham-router/src/runtime.rs`) — the
   doc said "Bayesian online changepoint detector"; the recursion was
   inert, only the vote-change branch moved the posterior. **COSMETIC** —
   the flag is opt-in and doesn't affect defaults.
3. **`posterior_variance`** — the doc says "the B2 confidence gate
   input"; no B2 gate exists. **COSMETIC** — the value is computed but
   never used, so nothing is worse for its absence. But downstream
   reasoning (the v3 §5.2 work, this session's audits, and future
   agents) treats it as if a defense-in-depth gate were in place.

The pattern is: **a function or field is written to be a gate input, and
then the gate is never implemented, but the doc-comment retains the
"this is a gate" language.** Type (1) is a real bug; types (2) and (3)
are honest-documentation bugs that mislead subsequent reasoning.

## What the fix would be (three options)

### Option A — remove the field (cheapest)

Delete `last_post_var`, `posterior_variance`, and the update site; update
the doc-comment. If nothing will consume it, don't compute it.

### Option B — implement the gate (what the doc promised)

Wire `posterior_variance()` into `weights_for_hand` to lower the effective
N0 or add robust weight when variance is high, gated behind a feature
flag. This is essentially what B2 was for. Costs a routing-model change
that must be A/B'd (v7's EXP-015 is the natural A/B site).

### Option C — say so honestly (moderate)

Leave the field, but change the doc-comment to: "computed for future B2
gating; currently unused." Change the runbook / SPECS references from
"confidence gate input" to "candidate confidence-gate input."

## Recommendation

**Option A if B2 is still not on the roadmap; Option C if it is.** Do
not leave the current wording: it's the third time an audit has found
"doc claims a gate, no gate exists" in this codebase, and each time
costs a session to re-discover. The RBP bug was caught only because
someone re-read the gate condition.

## What was NOT broken

- The `last_post_var` computation itself (Dirichlet marginal variance) is
  correct given the fusion model.
- `reset_session()` clears it properly.
- The `ChangepointShield`'s `effective_n0` wiring **is** live (4 callers,
  including one production path in `weights_for_hand`).
