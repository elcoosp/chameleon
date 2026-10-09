# D1 harness: `split_ranges` is not a range (2026-10-09)

## The finding

`crates/cham-agent/tests/d1_fullgame_vbr.rs::split_ranges` draws combos
lexicographically:

    fn take(pool: &[u8], n: usize) -> Vec<[u8; 2]> {
        for i in 0..pool.len() {
            for j in (i + 1)..pool.len() {
                out.push([pool[i], pool[j]]);
                if out.len() == n { break 'outer; }
            }
        }
    }

With `n = 30` and a 26-card pool, the first 25 combos all contain
`pool[0]`; only the last 5 contain `pool[1]`. The hero "range" is
therefore "card `pool[0]` paired with 25 others" plus a handful. Same
for villain on the other half-deck.

## What this invalidates

**Nothing, for the decisions this session made.** Every D1 run used the
same construction, so cross-bundle comparisons (agent-honest-19dim
5.77, tiny-full 7.65, real-full pending) are apples-to-apples, and the
pre-registered F-3 decision table applies unchanged.

**Something, for the absolute numbers.** "VBR = 5.77 bb" is not "the
shipped blueprint's exploitability." It is "the BR value when hero holds
those specific 30 combos." A different 30 combos would give a different
number. The magnitudes this session reported are real measurements of
a real conditional; they are not the unconditional exploitability.

**The negative boards** (real-full boards 1 and 5 at −0.95 bb) are
explained: on boards where the shared card is bad, hero's whole range
loses value, and the BR can dip below zero. That is not a walker bug.

## The fix

Draw combos uniformly (or with a spread construction — one combo per
rank class, or a seeded shuffle of the full C(26,2) list) instead of
lexicographically. Two lines:

    let mut rng = rng_from_seed(seed);
    let mut pool = all_combos_in_half(avail);
    fisher_yates(&mut pool, &mut rng);
    (pool[..n], other_pool[..n])

The seed must be fixed so the harness stays reproducible.

## What to do

1. **Let the running real-full D1 finish.** It uses the same range
   construction as every prior run, so its number is comparable to
   agent-honest-19dim and tiny-full. Applying the pre-registered F-3
   table to it is valid.
2. **After that**, fix `split_ranges`, re-run D1 on the two bundles
   whose numbers D1 decisions depend on (agent-honest-19dim, real-full),
   and record both numbers with a note that the earlier pair used the
   lexicographic construction.
3. **Do not** compare the pre-fix and post-fix numbers as if they were
   the same measurement.

## Related

This is the same class of bug as the walker's own reach-weighted mass
error (`53053f6`): a fix that is correct in shape but wrong about which
quantities enter. Here the walker is right, the harness is the thing
picking unrepresentative inputs.

## Status

Discovered 2026-10-09 while reviewing why real-full D1 shows negative
per-board VBRs. Not fixed yet — the running measurement must finish
first so its number stays comparable to the others.
