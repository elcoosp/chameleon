# F1 A/B: search ON is 5000 mb/seating worse than OFF — it is a mapping bug (2026-10-01)

## The measurement

Same bundle (`artifacts/agent-honest-19dim`, argmax routing). Search off
vs search on, 2500 deals/pair, 45 000 seatings total:

| routing | mean |
|---|---:|
| `--agent full` (search OFF) | **+8 277** |
| `--agent full --search` | **+3 264** |
| Δ | **−5 013** |

Per-opponent:

| opponent | OFF | ON | Δ |
|---|---:|---:|---:|
| arch:nit      | +2 393 | −1 860 | **−4 253** |
| arch:tag      | +4 856 | −686 | **−5 541** |
| arch:lag      | +7 627 | +701 | **−6 926** |
| arch:station  | +14 172 | +3 891 | **−10 281** |
| callbot       | +25 130 | +14 114 | **−11 016** |
| jamfix        | +4 787 | +4 787 | 0 |
| pnash         | +4 585 | +4 078 | −507 |
| famB:tag      | +3 494 | +2 770 | −723 |
| noisy:0.1:lag | +7 457 | +1 583 | **−5 874** |

**Search ON is worse against every opponent it fires against.**
`jamfix` is byte-identical — either that opponent never reaches a river
state where the search fires, or the two runs happened to agree exactly.
Either way, the search is not just unhelpful; it is actively harmful.

## The bug

`cham_search::Subgame::tree()` always constructs a tree rooted at a
**hero-acts-first** decision node, with actions
`[check, bet0.5, bet1, jam]`.

But at a real river decision the legal action set depends on the state:

- **Hero acts first** (`obs.to_call == 0`): legal set is
  `{check, bet, ...}`. The solver's labels roughly match.
- **Hero faces a bet** (`obs.to_call > 0`): legal set is
  `{fold, call, raise}`. The solver's labels — check, bet0.5, bet1,
  jam — have **no counterpart** for fold or call, and no honest way to
  be mapped onto a raise.

My `search_bridge::map_to_legal` tries anyway:

    "check"      -> Check slot (may not be legal — dropped by is_legal)
    "bet*"/"jam" -> aggressive slot (Raise)
    "fold"       -> fold slot (solver has none at the root)
    "call"       -> call slot (solver has none at the root)

So when the hero faces a bet on the river, the solver's root
distribution (which is over a fictional action set) is heuristically
collapsed onto whatever legal actions happen to match by name. The
result is a nonsense decision — the agent essentially picks a
pseudo-random action from a distribution that was never about the
current state.

Against a caller (station, callbot), the hero "faces a bet" often,
which is why the search hurts the most against those opponents.

## The correct fix

`try_solve` must only fire when the state **exactly matches the
solver's tree root**. The trigger conditions:

1. `obs.street == Street::River` (already enforced).
2. `obs.pot_bb() >= min_pot_bb` (already enforced).
3. **`obs.to_call == 0`** — the hero is FIRST TO ACT on the river. The
   solver tree assumes hero acts first; if the hero faces a bet, the
   solver's tree does not model the state.
4. The legal actions must be a superset of the solver's root labels —
   or the mapping must be provably lossless.

Add (3) to the trigger. This makes the search conservative but
correct: it fires only in states the subgame was built to model.

## What this does not fix

Even with (3), the subgame model is still too agnostic to be trusted:
- Hero is a single class (weight 1.0, strength = real equity). Fine.
- Villain is 3 uniform-weight classes spread over `[0.5 − 0.5, 0.5 + 0.5]`.
  That is not a real range.
- The bet grid is fixed `[0.5, 1.0]` pot fractions; the actual state
  may have different sizes.

Real gains require the villain range to come from the tracker and the
robust policy's reach. That is the follow-up work in
`F1-SEARCH-WIRED-2026-10-01.md` §"What F1 does not do".

## What to do now

1. **Add `obs.to_call == 0` to the trigger.** Rerun the A/B. If search
   ON now matches search OFF, the search is *safe* on its narrow
   domain but not yet *helpful*.
2. **Pull the `--search` flag from the shipped default.** It already is
   off — the point is to document loudly that it should stay off until
   the villain-range model improves.
3. **Do not run the search A/B again** until the subgame model is
   upgraded. Every measurement on the agnostic model is wasted.

## Artifacts

- `artifacts/ladder-f1-search-off.log`
- `artifacts/ladder-f1-search-on.log`
- `crates/cham-agent/src/search_bridge.rs` — the mapping bug
- `crates/cham-search/src/subgame.rs` — the tree root

## Related

- `F1-SEARCH-WIRED-2026-10-01.md` — the wiring that enabled this test
- `COMPETITIVE-REVIEW-2026-10-01.md` — finding F1
