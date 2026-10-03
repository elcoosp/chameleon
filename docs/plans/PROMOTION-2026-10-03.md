# Promotion: agent-honest-19dim now holds the retrained policies (2026-10-03)

## What changed

`artifacts/agent-honest-19dim` (the bundle the docs call "the shipped
bundle") was overwritten with the 2026-10-03 retrained policies:

- `robust/policy.bin`  -> DCFR(1.5, 0, γ=2), 5M, retrained
- `experts/{0,1,2,3}/policy.bin` -> retrained nit/tag/lag/station, 5M
- `router.bin` -> unchanged (reused from the previous bundle; the retrain
  did not re-fit the router)

The previous bundle is backed up at `artifacts/agent-honest-19dim-prev`.

## Verified

- All 6 policy files hash-match the retrained source
  (`agent-honest-19dim-retrained`).
- Ladder on the promoted bundle reproduces the retrained result: 9
  opponents, max |diff| 308 mb vs the pre-promotion retrained run
  (run-to-run ladder noise at 2500 deals/pair).
- `artifacts/*` is gitignored; no tracked files changed.

## The CLI default is a DIFFERENT bundle (open question)

`crates/cham-cli/src/cmd/hero.rs:98` defaults to **`artifacts/agent`**,
which is neither the promoted bundle nor the pre-promotion one:

| bundle | robust hash | router |
|---|---|---|
| `artifacts/agent` (CLI default) | ea3703a7 | none |
| `agent-honest-19dim-prev` (old shipped) | 58b82c90 | yes |
| `agent-honest-19dim` (now promoted) | cac471ea | yes |

So a bare `chameleon ladder --agent full` (no `CHAM_AGENT_BUNDLE`) uses
a router-less bundle that has never been measured in the retrain
comparison. **If "the shipped bot" means the binary's default, this
promotion does not reach it.** The retrained comparison was always run
with `CHAM_AGENT_BUNDLE=artifacts/agent-honest-19dim` explicitly.

### To actually change the default

Either:
1. Copy the promoted bundle into `artifacts/agent` (but `artifacts/agent`
   has no `router.bin`; the `--agent full` mode reads one — check
   whether a router-less default is intentional), or
2. Change the default path in `hero.rs` to `artifacts/agent-honest-19dim`,
   or
3. Leave the default and document that competitive runs must set
   `CHAM_AGENT_BUNDLE`.

This is a decision, not a mechanical step — flagged for the user.

## Retained evidence

- Retrain: `artifacts/retrain-2026-10-02/summary.txt`
- Retrained-bundle ladder: `artifacts/ladder-retrained-2026-10-03/summary.txt`
- Promoted-bundle verification: `artifacts/promote-verify-2026-10-03/promoted.log`
- Comparison doc: `RETRAIN-19DIM-RESULTS-2026-10-03.md`
- jamfix decomposition: `JAMFIX-REGRESSION-2026-10-03.md`
