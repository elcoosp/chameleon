# F6c — off-tree translation: implementation status (2026-10-01)

**Status:** primitives landed; pipeline wiring landed and gated OFF.
The A/B against the off-tree bettor pool (report E6) has not been run.

## What landed

### `crates/cham-engine/src/ladder.rs`

- `pub fn ph_prob_lower(a, b, x) -> f64` — Ganzfried–Sandholm 2013
  pseudo-harmonic mapping (probability of mapping real fraction `x`
  to the smaller bracketing abstract size `a`). Boundary-correct:
  `x = a` → 1.0, `x = b` → 0.0. Matches the report's reference:
  `A=0.5, B=1.0, x=0.6` → 0.750.
- `ActionLadder::translate(&self, obs, seq, real, u) -> Action` — maps
  an off-tree aggressive action to a same-class abstract slot using
  `ph_prob_lower`. Non-aggressive actions pass through. `u` is a
  caller-supplied deterministic uniform in `[0, 1)`.

### `crates/cham-agent/src/pipeline.rs`

- `fn offtree_translate_enabled() -> bool` — reads
  `CHAM_OFFTREE_TRANSLATE` once per process (`OnceLock`), returns
  `true` only for the exact string `"1"`. Default OFF.
- `fn derive_translate_u(hand_idx, street, lens) -> f64` — FNV-1a hash
  of `(hand_idx, street, seq.lens)`, top 53 bits → `[0, 1)`. Pure
  function; replay is bit-exact.
- `ChameleonAgent::on_public_action` — when the gate is on, translates
  the opponent's recorded action through `translate` before calling
  `encoder.record`. When off, behavior is identical to pre-F6c.

### Tests

- `crates/cham-engine/tests/f6c_translate.rs` — 5 unit tests pinning
  `ph_prob_lower` (boundaries, report example 0.750, monotonicity,
  range).
- `crates/cham-agent/tests/f6c_translate_u.rs` — a visibility marker
  documenting that the replay-determinism variant of the translate
  path is not yet tested. (The existing
  `cham-agent::pipeline_deterministic_replay` runs with the gate off.)

## Why the gate is OFF by default

The shipped bundle `artifacts/agent-honest-19dim` was **trained and
keyed without translation**. If inference silently translates off-tree
sizes before keying, the key stream the agent produces at runtime no
longer matches the training-time key stream, and every off-tree
decision falls back to uniform.

Enabling translation by default would therefore **break the shipped
bundle** until the bundle is retrained with the gate on. This is the
same class of error as the 2026-10-01 stale-binary trap (§7.1 of the
evening handoff): a runtime change that invalidates a training-time
keying invariant.

The gate is the safe staging: the code path exists, is exercised by
the primitives' tests, and can be enabled for a controlled A/B once
the A/B harness is ready.

## What remains

1. **Replay test with the gate on.** Add a variant of
   `pipeline_deterministic_replay` that sets
   `CHAM_OFFTREE_TRANSLATE=1` in the child process (not in the test
   process, to avoid polluting other tests). Assert bit-identical
   sequences across two replay passes.
2. **E6 A/B.** Build an off-tree bettor pool (opponents that bet
   0.25 / 0.6 / 1.5 pot on flops/turns/rivers). Measure:
   - fallback rate (key not found → uniform) before/after translation;
   - ladder result (mb/seating) against that pool before/after.
   The report's gate: fallback < 2% and ladder improvement positive.
3. **Retrain with the gate on.** If (2) succeeds, retrain the shipped
   bundle under `CHAM_OFFTREE_TRANSLATE=1`. This is a multi-hour
   retrain; the corrected metric (`tabular_br`) must be the measured
   metric, not `lbr_vs`.
4. **size_bucket re-quantization.** The report's second fix (line 402):
   quantize `size_bucket` from the abstract slot index, not from the
   stack fraction. This is a keying change and cannot ship without the
   retrain from (3). Design not yet written.

## Relationship to F10

F6c addresses the *translation* half of "the action abstraction is not
a poker game." F10 addresses the *search* half (a vector-form river
solver). The report's roadmap puts both under Phase 2 ("Real tree"),
which is the multi-day job the evening handoff deferred.
