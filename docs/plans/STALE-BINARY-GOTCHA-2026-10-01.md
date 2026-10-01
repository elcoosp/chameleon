# Stale release binary caused a false CallBot result (2026-10-01)

## What happened

Ran `ladder --fast --agent sample-expert` on the 19-dim SOTA bundle
immediately after the F7 commit (`1045657` at 13:20). The result was
byte-identical to the CallBot-vs-pool baseline measured for
`full-hedged` on 2026-09-30:

| opponent | sample-expert (this run) | CallBot-vs-pool (Sep 30) |
|---|---:|---:|
| arch:nit      | −1 299 | −1 266 |
| arch:tag      | −1 324 | −1 373 |
| arch:lag      | −4 503 | −4 528 |
| arch:station  | −480   | −472 |
| callbot       | **+0** | **+0** |
| famB:tag      | −2 690 | −2 690 |
| noisy:0.1:lag | −3 997 | −3 832 |

The `callbot: +0.0` is the CallBot-vs-CallBot mirror-match signature.
The ladder was silently measuring CallBot, not the 19-dim honest
router.

## Root cause

`target/release/chameleon` mtime was **Sep 30 20:38**. F7 landed
**Oct 1 13:20**. The ladder invocation used the stale binary, whose
`TRAINED_AGENTS` array did NOT include `"sample-expert"`. So
`ladder.rs::run_opponent` fell through:

    let trained = guard::requires_trained_artifacts(agent);  // false in old binary
    let mut hero = if trained { build_chameleon(...) } else { CountingHero::new(CallBot) };

Silent fall-through — no warning printed. The M-6 warning pattern is
exactly this: hardcoded guard lists that silently degrade.

## The fix

Rebuild release after every commit that touches `guard.rs`,
`hero.rs`, `modes.rs`, or `pipeline.rs` routing code. Better: make
`ladder.rs` refuse if the requested agent name isn't in
`routable_agents()` (the set of names `routing_for` accepts), so a
typo or a stale binary produces a loud error rather than CallBot.

## The lesson (again)

Two incidents now:
1. 2026-09-30: `full-hedged` missing from TRAINED_AGENTS → CallBot
2. 2026-10-01: stale release binary missing `sample-expert` from the
   new TRAINED_AGENTS → CallBot

Both produced silent CallBot measurements that looked like real
results. The 2026-09-30 fix added a regression test in `guard.rs`:
`every_routable_agent_requires_artifacts`. That test would have caught
BOTH bugs — but only if the release binary was rebuilt.

**Recommendation for the next session:** add
`chameleon ladder --agent <name>` validation that:
1. Checks `name` against the union of `hero::routing_for` aliases and
   `guard::TRAINED_AGENTS`.
2. Refuses with a clear error if the name isn't recognized.
3. Prints the release binary's build hash so a stale binary is
   visible in the pipeline log.

## Artifacts

- `artifacts/ladder-19dim-sample-expert.log` — the false result (to be overwritten)
- `artifacts/ladder-19dim-argmax.log` — the true argmax result (+8 271)
