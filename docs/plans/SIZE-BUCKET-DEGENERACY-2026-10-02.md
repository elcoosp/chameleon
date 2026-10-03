# size_bucket is degenerate: the infoset key carries almost no size information (2026-10-02)

**Finding.** `record_action`'s `size_bucket` — the size-bearing component
of the infoset key — carries **one bit** of resolution: *jam vs
normal-sized*. It does not distinguish normal sizes from each other.

Two measurements pin this:

- Sampled real actions (`size_bucket_distribution.rs`, aggressive-only,
  jams skipped): **100% land at bucket 1.**
- Enumerated slots at a flop node (`slot_bucket_range.rs`): the two
  aggressive slots `Bet{to:100}` (0.5 pot) and `Bet{to:9900}` (jam)
  produce buckets `{1, 12}`.

So the key separates a jam from a non-jam and nothing finer. (An earlier
draft of this doc said "always 1"; that was wrong — it ignored the jam
slot, which the sampler had explicitly skipped.)

## Measurement

`crates/cham-engine/tests/size_bucket_distribution.rs` walks 2000 real
hands (tiny abstraction, 100bb, seed 0..2000), taking an aggressive
action at every other opportunity so hands progress through streets.
Over 8000 aggressive actions:

    bucket 1: 8000 (100.0%)

    by street (aggressive / bucket<=1):
      pre:   2000, 2000 (100.0%)
      flop:  2000, 2000 (100.0%)
      turn:  2000, 2000 (100.0%)
      river: 2000, 2000 (100.0%)

Not a single action reached bucket 2.

## Why

`size_bucket = round(sf * 12).clamp(1, 15)` with
`sf = (to - current_bet) / effective_stack`.

The clamp floor of 1 absorbs every small `sf`. The bucket leaves 1 only
when `sf * 12 >= 1.5`, i.e. the action is **>= 12.5% of the effective
stack**. On a 100bb stack that is a 12.5 bb action.

The tiny ladder's postflop sizes are *pot-relative* (flop/turn 0.5 pot,
river 0.5/1.25 pot). A 0.5-pot bet is only 12.5% of stack once the pot
reaches 25 bb. Early streets and normal-sized pots never get there, so
every normal-size bet saturates at the clamp floor (1); only a jam
escapes to a higher bucket. The sampler confirms no normal-size bet
reached bucket 2 in 8000 samples.

## Consequences

1. **The E6 translation no-op is explained.** `translate` maps an
   off-tree size to an on-tree slot, but the recorded `size_bucket` is
   1 before and after. Translation cannot change a key whose size
   component is already constant
   (`F6C-E6-FALLBACK-MEASUREMENT-2026-10-02.md`).

2. **`CHAM_SLOT_BUCKET` is the only fix that can matter.** Slot-index
   bucketing (`slot + 1`) gives distinct buckets per abstract slot; it
   is the only variant that makes the key size-aware. This is why the
   report couples the translation fix with the bucket fix.

3. **The abstraction is coarser than its config says.** `abstraction.toml`
   lists several bet fracs per street, implying those sizes are part of
   the state space. With the size bucket constant, histories that differ
   only in bet size collide into one key. The effective abstraction is
   smaller — and less expressive — than the config suggests.

4. **This is a *second* key degeneracy, independent of F6c.** Even with
   `CHAM_SLOT_BUCKET` off and no translation, the current shipped bundle
   is trained on keys where size information is collapsed. A retrain with
   the slot bucket on (the retrain launched 2026-10-02) will be the first
   to see a size-aware key space.

## Caveats

- The sampler takes "the first non-jam aggressive slot", not a
  size-distribution-weighted draw. A policy that bets all-in often would
  reach higher buckets (jam is `sf` large). The point is about
  *normal-sized* bets, which are what the ladder mostly produces.
- 100bb / tiny / seed 0..2000. Deeper stacks would raise `sf` at fixed
  chip sizes but the pot-relative ladder scales with the stack, so the
  conclusion is depth-robust for pot-relative sizing.

## Follow-up

- After the slot-bucket retrain, re-run this sampler with
  `CHAM_SLOT_BUCKET=1` and confirm the distribution spreads across
  `1..n_slots`. If it does, the key is size-aware.
- Consider whether the clamp floor of 1 should be lower (0), so that
  sub-12.5%-stack actions are distinguishable from the 12.5%+ ones. That
  is a separate keying change; the slot bucket supersedes it.

## The deeper limit: the ladder has ~1 size per street

`crates/cham-engine/tests/slot_inventory.rs` enumerates the aggressive
slots the tiny ladder offers, sampled over 300 hands:

| street | normal aggressive sizes | jam |
|---|---:|---:|
| preflop | 1 | 1 |
| flop | 1 | 1 |
| turn | 1 | 1 |
| river | 2 | 1 |

So the tiny ladder does not merely *quantize* sizes coarsely — it has
essentially **one** normal bet size per street (two on the river). The
`size_bucket` degeneracy is a symptom of this: with one size, there is
nothing for the bucket to distinguish.

### What this means for the fixes

- **Slot-index bucketing (`CHAM_SLOT_BUCKET`) buys at most 1 bit on
  tiny.** With one normal slot + one jam, the slot bucket yields buckets
  {normal, jam} — the same 2 values the stack-fraction bucket already
  produces. On the river (2 normals + jam) it yields 3. That is the
  ceiling; it cannot make the key "size-rich" because the ladder has no
  size richness to encode.

- **The real lever is a richer ladder**, not a better bucket. The
  `config/abstraction-tiny-rich.toml` (bet fracs `[0.33, 0.75, 1.5]`
  postflop, cap 2) is the config that would give the bucket something to
  do. F6c's translation machinery only pays off on such a ladder.

- **The retrain launched 2026-10-02 is correctly scoped as a
  same-ladder control** (it isolates the trainer fixes from any ladder
  change). A *second* retrain on `abstraction-tiny-rich` — with the slot
  bucket on — is the experiment that would actually test whether size
  resolution helps. That is the report's Phase 2 "Real tree" work.

### Revised recommendation

1. Keep the slot-bucket + translation code (it is correct and gated).
2. Do **not** expect it to move the tiny numbers — it cannot, by the
   inventory above.
3. The next real experiment is `abstraction-tiny-rich` + slot bucket,
   measured with the corrected metric. That is a ladder change, a
   retrain, and a fresh curve — the report's Phase 2.

## The rich ladder: more sizes, same collapse

`crates/cham-engine/tests/ladder_inventory_compare.rs` compares the tiny
and `abstraction-tiny-rich.toml` ladders directly.

**Distinct normal aggressive sizes per street:**

| street | tiny | rich |
|---|---:|---:|
| preflop | 2 | 2 |
| flop | 1 | 3 |
| turn | 1 | 3 |
| river | 2 | 3 |
| **total** | **6** | **11** |

So the rich ladder *does* offer more sizes — "use the rich ladder" is a
real lever (11 vs 6 distinct sizes).

**But the stack-fraction bucket collapses them anyway.** At a flop node
the rich ladder's three normal sizes are 100 / 150 / 300 chips
(0.33 / 0.75 / 1.5 pot). All three map to stack-bucket **1**:

    rich flop slots (to, stack_bucket, is_jam):
      (100, 1, false), (150, 1, false), (300, 1, false), (9900, 12, true)

Three distinct sizes, one bucket. The bucket does not distinguish them.

### The complete chain

Size resolution in the infoset key requires **both**:

1. **A richer ladder** (more distinct sizes) — `abstraction-tiny-rich.toml`
   gives 11 vs tiny's 6.
2. **The slot-index bucket** (`CHAM_SLOT_BUCKET=1`) — the only bucketing
   that maps distinct slots to distinct buckets. With the rich ladder's
   3 flop slots, slot+1 = 1/2/3, so the key finally separates them.

Neither alone is sufficient:

- Rich ladder + stack-fraction bucket → 3 sizes collapse to bucket 1
  (this test).
- Tiny ladder + slot bucket → only 1 normal slot to distinguish, so it
  buys ~1 bit (`SIZE-BUCKET-DEGENERACY` §ladder-inventory).

### Consequence for the roadmap

The experiment that would actually test size resolution is:

    abstraction-tiny-rich.toml  +  CHAM_SLOT_BUCKET=1  +  retrain

measured with `tabular_br`. That is the report's Phase 2 "Real tree"
work, now with a precise, measured justification for both halves of the
change.
