# F6c — size_bucket re-quantization design (2026-10-01)

**Status:** design only. No code landed. This is a **keying change**
and cannot ship without a retrain (see section 4).

## 1. The problem

`record_action` in `crates/cham-engine/src/ladder.rs` computes the
`size_bucket` field of every `SeqEntryRaw` from the **stack fraction**
of the real action:

    let sf = ladder.stack_frac_of(obs_before, a);
    let bucket = if matches!(a, Fold | Check | Call) {
        0u8
    } else {
        ((sf * 12.0).round() as i64).clamp(1, 15) as u8
    };

`stack_frac_of` is `bet / effective_stack`. Two consequences:

1. The bucket is a function of the *real* action, not the *abstract
   slot* the action maps to. Two real sizes that map to the same
   abstract slot but have different stack fractions (e.g. a 40 bb
   raise and a 55 bb raise both clamping to the "raise ~ pot" slot)
   produce **different keys**.
2. At inference, the opponent's real action is recorded verbatim
   (`on_public_action` before F6c wiring). If the opponent picks a
   size the training tree never produced, the bucket likely differs
   from any training-time bucket — the key misses the table and the
   agent falls back to uniform. The report measured 17-27% miss rates
   on the tiny pool.

## 2. The fix (report section F6c, line 402)

> Also quantize `size_bucket` from the **slot index**, not from the
> stack fraction, so the bucket is a function of the abstract action
> alone. Both training and inference then produce the same key by
> construction.

The slot index is the position of the abstract action in the ladder
returned by `ActionLadder::slots(obs, seq)`. It is stable: it depends
only on the ladder config and the abstract action itself, not on the
real bet size the engine happened to execute.

### New `record_action` shape

    pub fn record_action(
        ladder: &ActionLadder,
        obs_before: &Observables<'_>,
        actor: Player,
        a: Action,
        seq: &mut ActionSeq,
    ) {
        let class = /* unchanged */;
        let bucket = match a {
            Action::Fold | Action::Check | Action::Call => 0u8,
            _ => {
                let slot = ladder
                    .slots(obs_before, seq)
                    .iter()
                    .position(|s| s.action == a)
                    .map(|i| i as u8)
                    .unwrap_or(0);
                (slot + 1).min(15)
            }
        };
        seq.push(obs_before.street, SeqEntryRaw {
            actor: actor.as_usize() as u8,
            class,
            size_bucket: bucket,
        });
    }

### Why bucket = slot + 1

- Slot 0 in the "facing no bet" case is `Check`; in the "facing a bet"
  case is `Fold`. Both map to bucket 0 via the Fold/Check/Call arm, so
  those slots never reach the aggressive arm.
- Aggressive slots start after the leading passive/fold slot. Using
  `slot + 1` avoids colliding with bucket 0.
- Clamping to 15 matches the existing field width. The ladder is capped
  at 12 slots (`ArrayVec<AbstractAction, 12>`), so `slot + 1 <= 12` in
  practice; `.min(15)` is defensive.

### Interaction with `ActionLadder::translate`

`translate` (landed earlier this session, `5f1d708`) maps an off-tree
real action to a same-class abstract slot when `CHAM_OFFTREE_TRANSLATE`
is on. With translate + slot-index bucketing:

- Off-tree real action -> `translate` picks a slot -> `record_action`
  sees an abstract action that IS in the ladder -> `position` finds
  its slot -> bucket = slot + 1.
- On-tree real action -> same path, deterministic.

Both paths produce the same key for the same abstract history. This is
the "by construction" property the report asked for.

## 3. Tests that would pin the change

1. **Slot-index stability.** For a fixed ladder and a fixed sequence
   of abstract actions, `record_action` must produce the same
   `size_bucket` regardless of the real bet amount that maps to the
   same slot. Test: two `Action::Raise { to: X }` and
   `Action::Raise { to: Y }` where both clamp to the same slot in a
   test ladder must produce identical `SeqEntryRaw`.
2. **Round-trip with translate.** Feed an off-tree action through
   `translate` then `record_action`; the resulting seq must equal the
   seq obtained by feeding the corresponding abstract action directly.
3. **No bucket-0 collision.** Assert that `size_bucket == 0` iff the
   action is Fold/Check/Call, for every abstract action the ladder
   ever produces on a sampled state.
4. **Determinism across runs.** Two runs of the same hand with the
   same RNG seed must produce identical seq bytes. (Partially covered
   by the existing `pipeline_deterministic_replay`; needs a variant
   with the new bucketing.)

## 4. Why this cannot ship without a retrain

The shipped bundle `artifacts/agent-honest-19dim` was trained with the
**old** stack-fraction bucketing. Its table keys are indexed by the
old buckets. If inference switches to slot-index bucketing, every key
produced at runtime differs from the training keys, and the agent
falls back to uniform for **every** decision — a total regression.

The retrain is required and is the same "multi-hour, corrected-metric"
job the F6c doc describes. Sequencing:

1. Land the code change behind a build-time or env-time gate (like
   `CHAM_OFFTREE_TRANSLATE`).
2. Retrain a candidate bundle with the gate on.
3. Measure both old and new bundles on the same opponent pool with
   `tabular_br` (corrected metric), not `lbr_vs`.
4. Only if the new bundle is not worse (and the off-tree pool
   fallback rate drops toward 0 per the report's E6 gate) does the
   new bundle become the shipped one.

## 5. What is NOT this document

- The ladder *cap* fix (F6a — `raises_this_street` counting bets as
  raises) is separate and has already landed (`7a68ac7`).
- The `preflop_open_bb` dead config (F6b) is separate and cosmetic.
- The pseudo-harmonic translation itself is separate and has landed
  (`5f1d708` + `04da4a8`), gated off.
