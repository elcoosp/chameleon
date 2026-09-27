# Artifact key-format break (2026-09-27)

## What happened

The L-1 fix (commit `2b3c5f4`, completed by `7e0621e`) added a per-street
`overflow: [u8; 4]` counter to `ActionSeq` and folded it into the `key_for`
byte stream. This was the correct fix for a real key-collision bug: two
histories that share the first 8 actions of a street but diverge afterward
must produce different infoset keys.

**But it changed the key format.** Every `policy.bin` trained before that
commit stores rows keyed by the OLD format (no overflow byte). Loading such
an artifact against the current encoder produces zero key matches:
`fallback rate: 100.0%` in `probe --diag-fallback`.

The regression was not caught by the test suite because no test compares a
pre-L-1 artifact against a post-L-1 encoder.

## Impact

Every pre-existing bundle is invalidated for load-and-play:

- `artifacts/agent`            (trained 2026-09-26 19:28)
- `artifacts/agent-honest`     (trained 2026-09-27 13:35)
- `artifacts/agent-full`       (trained 2026-09-26 14:52)
- All `artifacts/blueprints-*/` subdirectories

Diagnostics that consult the trained weights (probe, ladder, play, ab) will
report 100% fallback until the artifacts are retrained.

## Why this was not caught earlier

1. `sb_dump` and the diagnostic paths were pointed at stale fixtures, so
   nobody noticed the mismatch.
2. The `abstraction_hash` guard only covers the abstraction config + bucket
   bytes, not the key stream — it cannot detect a key-format change.
3. No test pins key bytes across a change to `key_for`.

## Remediation options

1. **Retrain from scratch.** Correct but expensive (~2h per tiny bundle,
   longer for full).
2. **Revert the L-1 key-format change** and find another disambiguation
   (e.g. a per-street collision counter checked only at match time). Not
   recommended: reintroduces the underlying bug.
3. **Version the key stream.** Add a `KEY_VERSION` byte to the leading
   stream; encoders that see an older version convert or reject with a
   specific error. Do this BEFORE retraining so future key changes are
   visible at load time, not silent.

## Follow-up (must-do)

Add a **cross-version key regression test**:

```rust
#[test]
fn key_format_is_pinned() {
    // Build a canonical ActionSeq, compute a key, assert the first N bytes
    // of the byte stream match a committed golden. Any change to the key
    // stream must update the golden and trigger a rebuild of every artifact.
}
