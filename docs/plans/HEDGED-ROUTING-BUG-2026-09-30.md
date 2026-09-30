# Hedged routing is a disaster on the archetype pool — and there was a bug (2026-09-30)

**CORRECTION 2026-09-30 (13:21):** The full-hedged numbers in this doc
were produced by a `ladder` invocation that silently fell through to
CallBot (the CLI's `TRAINED_AGENTS` guard did not include
`full-hedged`). The numbers reflect CallBot-vs-pool, not hedged
routing. See `HEDGED-BUG-TRAINED-AGENTS-2026-09-30.md`. A re-measurement
with the fix is at `artifacts/ladder-full-hedged-FIXED.log`.

## The measurement

`CHAM_AGENT_BUNDLE=artifacts/agent-honest chameleon ladder --fast --agent full-hedged`:

| opponent | full-hedged | full (argmax) | robust-only |
|---|---:|---:|---:|
| arch:nit      | −1 266 | +1 384 | −728 |
| arch:tag      | −1 373 | +3 382 | −947 |
| arch:lag      | −4 528 | +3 932 | −1 368 |
| arch:station  | −472   | +14 259 | +172 |
| callbot       | +0     | +24 962 | +7 231 |
| jamfix        | −299   | +4 787 | +4 266 |
| pnash         | −362   | +4 168 | +2 646 |
| famB:tag      | −2 690 | +2 269 | +583 |
| noisy:0.1:lag | −3 832 | +5 084 | −659 |
| **mean**      | **−1 800** | **+6 587** | **+720** |

Hedged is worse than robust-only by ~2 500 mb/seating and worse than
full by ~8 400. SPRT stopped at AcceptH0 on 5 of 9 opponents — a
definitively negative result, not noise.

## Bug found: modes.rs::AgentMode::validate didn't accept "hedged"

`loader.rs::load_agent` and `pipeline.rs::new` both call
`AgentMode::validate`, which matched only
`"mixture" | "argmax" | "robust-only" | "bayes"`. `hero.rs::routing_for`
maps `"full-hedged"` and `"hedged"` to the routing string `"hedged"`,
so any code path that validates the mode refuses to build the agent.

Caught by `chameleon probe --diag-fallback --agent full-hedged`:

    probe --diag-fallback: agent: loader: unknown routing: hedged

Fixed in commit 88b8b4a: add "hedged" to the validate() match arm and
add AgentMode::hedged() for parity with argmax() and robust_only().

Impact: the ladder measurement still produced a number — because the
ladder path (`hero::build_chameleon`) constructs an AgentMode in place
without calling validate(). Two construction paths exist; one validates,
one does not. Confusing and worth cleaning up in a future session.

## Why hedged loses (structural, not just a bug)

`pipeline.rs::act_impl` hedged branch:

    let top = argmax_k.unwrap_or(0);
    let top_weight = weights[top];
    if top_weight >= threshold {
        // confident: play top expert purely (argmax-style)
    } else {
        // uncertain: fall back to the mixture
    }

`weights` is a Dirichlet-multinomial posterior over the four experts,
not a per-hand router confidence. It accumulates session vote counts.
Early in a session, `weights[top]` is close to the router's sharpened
prior (~0.3-0.5). Late in a session, if the router has voted
consistently, `weights[top]` converges to ~1.0.

With threshold 0.5, hedged therefore plays **mixture early, argmax
late** in every session, with a phase transition somewhere in the
middle. That is not the design intent — the design intent is "argmax
when the router is confident on this hand, mixture otherwise", a
per-hand decision.

## The threshold sweep (running)

`scripts/ladder-hedge-sweep-2026-09-30.sh` runs `full-hedged` at
thresholds 0.00, 0.20, 0.50, 0.80, 1.00. The endpoints give decisive
diagnostics:

- threshold=0.00 should equal `full` (+6 587). If not, the hedged
  decision path differs from the argmax decision path in some other
  way — a second bug.
- threshold=1.00 should equal `full-mixture`. If not, the hedged
  fallback path differs from the standalone mixture path.
- Intermediate thresholds reveal the phase transition point.

## Suggested fix (not implemented)

Make the hedge per-hand, not per-session. Use the router's sharpened
prior entropy (before the Dirichlet update) or the posterior variance
`last_post_var` as the uncertainty signal.

## Big picture — this doesn't change the frontier

The shipping configuration is still `agent-honest` with `--agent full`
(argmax+synthetic, +6 587). Hedged was an experimental routing mode.
This finding confirms the SOTA doc.

## Artifacts

- `artifacts/ladder-agent-full-honest-full-hedged.log`
- `artifacts/ladder-hedge-thr*.log` (in progress)
