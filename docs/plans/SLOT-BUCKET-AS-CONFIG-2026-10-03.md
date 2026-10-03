# Design: slot-index bucketing as config, not env (2026-10-03)

**Status:** design only. NOT implemented — the F6c rich retrain is
running and a source change would risk its final `cargo nextest` step.

## The problem

`CHAM_SLOT_BUCKET=1` is a **process-global env var** read by
`ladder::slot_bucket_enabled()` (`crates/cham-engine/src/ladder.rs`). It
selects whether `record_action` keys `size_bucket` from the slot index
or the stack fraction.

Consequences:
- A bundle trained with it ON is **not self-describing**. Load it in a
  process without the env var and every key mismatches → 100% fallback.
- It is **not part of `abstraction_hash`** (which hashes the TOML bytes
  + bucket artifacts). Two runs that produce incompatible keys have the
  *same* hash if the TOML is unchanged.
- So the F6c rich bundle (`artifacts/agent-f6c-rich`, retraining now) is
  a **research artifact**: usable only if every consumer sets the env
  var. It cannot be shipped as-is.

## The fix

Move the flag into `LadderConfig` so it is (a) self-describing and
(b) hash-covered.

### Change 1 — `LadderConfig` field

```rust
pub struct LadderConfig {
    // ... existing ...
    /// F6c (2026-10-03): key `size_bucket` from the abstract slot index
    /// rather than the stack fraction. `false` = historical behavior.
    #[serde(default)]
    pub slot_bucket: bool,
}
```

`#[serde(default)]` → existing TOMLs (no `slot_bucket` key) parse
identically, and — because `abstraction_hash` hashes the **raw TOML
bytes** — their hashes are UNCHANGED. Only TOMLs that add
`slot_bucket = true` get a new hash. Backward compatible.

### Change 2 — thread it to `record_action`

`ActionLadder::new(cfg: &AbstractionConfig)` already stores `cfg`. Add:

```rust
impl ActionLadder {
    pub fn slot_bucket(&self) -> bool { self.cfg.ladder.slot_bucket }
}
```

Then `record_action` (which takes `ladder: &ActionLadder`) replaces:

```rust
} else if slot_bucket_enabled() {
```

with:

```rust
} else if ladder.slot_bucket() {
```

and `slot_bucket_enabled()` (the env reader) is deleted.

### Change 3 — the configs

- `config/abstraction-tiny.toml` — unchanged (defaults to false,
  preserving the shipped bundle's keys).
- `config/abstraction-tiny-rich.toml` — add `slot_bucket = true`.
- Future: a `config/abstraction-tiny-rich-slot.toml` if we want both
  variants hash-distinct.

### Change 4 — CLI

No CLI change: the config carries it. `train-bp`/`ladder`/`probe` read
the config from `--config`/the bundle's `abstraction.toml`. The env var
`CHAM_SLOT_BUCKET` becomes a **deprecated no-op** (or a loud error if
set, to avoid silent confusion). Recommended: keep reading it ONLY as a
test hook, log a deprecation warning.

## Why not just keep the env var + document it?

Because "ship a bot that only works if you remember an env var" is the
exact class of silent-key-mismatch bug this whole session has been
fixing (stale binaries, F4 truncation, translation no-op). A
self-describing bundle is the correct design; the env var was a
staging mechanism.

## Migration

1. Land changes 1-4 (source, compile, test — do NOT do this while the
   rich retrain runs).
2. The F6c rich bundle currently retraining is **env-gated**; re-run it
   after the config change (or accept it as research-only). Cheap: the
   experts are ~1.5h each; a config re-run is the same cost.
3. Verify: `abstraction_hash` of the rich TOML changes when
   `slot_bucket = true` is added, and the shipped tiny hash is
   unchanged.

## Tests to add

- `record_action` with a `slot_bucket=true` ladder produces slot+1
  buckets; with `false`, stack-fraction buckets. (Adapt
  `tests/slot_bucket.rs`.)
- `parse_config` of a TOML with and without the key → `slot_bucket`
  false/true, and hashes differ only in the second case.
- The shipped tiny bundle still loads + ladders unchanged (hash stable).

## Supersedes

The env-var gate (`CHAM_SLOT_BUCKET`) added in `e14cf6b` (F6c,
2026-10-02). That commit's own comment already flagged this as
"Phase 2" — this doc is Phase 2.
