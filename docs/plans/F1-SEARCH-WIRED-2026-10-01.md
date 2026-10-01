# F1 complete: river search is wired into the live decision path (2026-10-01)

## What landed

The pipeline now actually calls the river solver. Prior to this
change, `cham_search::solve` was invoked only by tests and one bench
— the shipped agent had an `AgentMode.search` field that was never
read on the hot path, and `DecisionTrace.search` was hardcoded
`None`. See `COMPETITIVE-REVIEW-2026-10-01.md`, finding F1.

### The changes

- **`crates/cham-agent/src/search_bridge.rs` (new)**: assembles a
  river subgame from the current `Observables` + tracker-derived
  model, invokes the (now class-conditioned, F2) solver, returns the
  chosen action + a `DecisionTrace.search` payload.

- **`crates/cham-agent/src/pipeline.rs`**: `act_impl` now calls
  `SearchBridgeCfg::from_mode` and, when the search is enabled and
  the trigger preconditions are met, invokes `try_solve`. On a
  successful (non-truncated) solve, the solver's class-conditioned
  choice **overrides the current action**. The reach update and trace
  then proceed against the overridden action, keeping state
  consistent.

- **`crates/cham-agent/src/modes.rs`**: the L-19 "not yet wired"
  refusal was relaxed. The former comment ("pipeline hardcodes
  `search: None`") is no longer accurate. The remaining G4 lockout
  (SPECS/06 §7) still requires a non-empty `g4_ledger_ref` on any
  enabled search.

- **`crates/cham-cli/src/cmd/play.rs`** and **`main.rs`**: added a
  `--search` flag. Passing it enables live river solving for the
  session, using `EXP-SEARCH` as the auditable G4 ledger token.

### The wiring tests

`crates/cham-agent/tests/search_bridge.rs`:

- **`no_search_when_flag_off`**: with `search.enabled = false` (the
  default), a river decision records `DecisionTrace.search = None`.
- **`search_fires_when_flag_on`**: with `search.enabled = true` and a
  valid `g4_ledger_ref`, the same river decision records
  `DecisionTrace.search = Some(...)`.

Both pass. The tests do not judge the solver's output quality — that
is the solver suite's job (see the one-card-poker acceptance test
shipped with F2). The point is that the bridge is reached.

## What F1 does NOT do (deliberately)

### The subgame build is a first-pass agnostic model

Hero is a single class (weight 1.0, strength = hero's river equity
via `strength_now`). Villain is K uniform-weight classes spread
uniformly over `[0.5 - spread/2, 0.5 + spread/2]` with
`spread = 1.0`. The tracker's frequencies are **not yet consumed**
by the bridge — a follow-up will build a villain range from the
tracker model and the robust policy reach.

### The bet-size grid is fixed

`[0.5, 1.0]` pot fractions. The full tree in `Subgame::tree` also
emits a `jam` action. A future pass may expose the bet grid as a
config.

### The bridge is opt-in

Every shipped bundle has `search.enabled = false`. Enabling requires
the explicit `--search` flag (or a bundle config that sets it). This
keeps all existing measurements valid — a search-enabled run is a
distinct experimental configuration, and its ladder/LBR numbers
should be compared to the non-search baseline.

## What this enables

The pipeline can now be run with `--search` on a river-heavy
scenario and the solver's decision appears in the trace. The next
measurement is a **ladder A/B** (`--search` on vs off) on the 19-dim
SOTA bundle. Expected: a small but real gain on unmodeled spots
(the pool's scripted opponents rarely exploit our blueprint's
unmodeled river lines, so the ceiling is modest).

## Cost

The bridge is called at most once per hero river decision. The
solver runs 400 iterations of RNR on a small tree (2 hero classes ×
3 villain classes × ≤ 5 root actions). Measured latency in the
existing `decision_latency_search` bench: **13.15 ms**. This is well
within a 250 ms move clock and matches the review's F1 estimate.

## Next steps

1. **Ladder A/B on the 19-dim bundle**: `--search` off vs on.
   Registers a real, shippable gain if it exists; registers "no
   gain" honestly if not.
2. **Villain range from tracker + robust reach**: replace the
   uniform-spread model with a real range built from
   `Tracker::raw_opponent_frequencies` and the robust policy's row
   values.
3. **Bet grid from config**: pass the current abstraction's
   `raise_fracs` / bet sizes rather than the hardcoded `[0.5, 1.0]`.

## Artifacts

- `crates/cham-agent/src/search_bridge.rs` — the bridge
- `crates/cham-agent/tests/search_bridge.rs` — the wiring tests
- `crates/cham-cli/src/cmd/play.rs` — the `--search` flag

## Related

- `F2-COMPLETE-2026-10-01.md` — the class-conditioned solver rewrite
- `F2-CLASS-CONDITIONED-SOLVER-PLAN-2026-10-01.md` — the plan
- `COMPETITIVE-REVIEW-2026-10-01.md` — the review that flagged F1
