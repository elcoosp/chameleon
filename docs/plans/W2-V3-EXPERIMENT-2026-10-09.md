# W2 experiment: v3 keys + real-full buckets

## What this is

The plan's W2 is "key v2, abstraction v3, buckets". The C-3 key split
(`key_for_public` + `key_from_public`) is now exposed (commit `55985a3`).
This experiment trains the first **v3** bundle:

- config: `config/abstraction-v3-nocompress.toml` (version=3,
  `slot_bucket=true`, `compress_history=false`)
- buckets: `artifacts/buckets-real-full` (reused — bucket tables are
  content, not version-dependent)
- 15M iters, seed 7, DCFR alpha=1.5 beta=0.0, 4 threads hogwild

Result goes to `artifacts/levers-bp/real-full-v3/robust/`.

## What it answers

The v3 vs v2 A/B on **matched abstraction**. Same buckets, same tree,
same iters, same seed. Only the key derivation differs:

- v2: bucket hashed inline in the byte stream (`fnv1a(&bytes[..n])`)
- v3: `fnv1a(public_with_bucket_zeroed) ^ bucket_mix(bucket)`

If the v3 key is a strict refactor (same equivalence classes of
infosets), the two bundles should have identical VBR. If v3's
slot_bucket=true changes key granularity, the two differ — and the
diff is the value of W2's key change.

## The comparison it enables

| bundle | keys | buckets | trained |
|---|---|---|---|
| real-full | v2, slot_bucket via env | 128/64/64/8 | done (15M) |
| real-full-v3 | v3, slot_bucket in config | 128/64/64/8 | in flight |

Both D1'd with the same harness, 60-180 boards.

## Why it matters beyond A/B

1. **v3 keys are config-sourced** — no `CHAM_SLOT_BUCKET` env trap (the
   bug that cost this session a garbage D1 run on `agent-honest-19dim`).
2. **The C-3 split is what the PCS walk needs** to replace per-combo
   hashing with a bucket table — the plan's stated perf primitive.
3. If v3 ≈ v2, W2's bucket fineness is what matters, and the next
   experiment scales bucket count. If v3 differs materially, the key
   change itself moved something.

## Command

    CHAM_AVG_DELAY=0 chameleon train-bp \
        --mode robust --iters 15000000 --depth 100 --seed 7 \
        --config config/abstraction-v3-nocompress.toml \
        --buckets artifacts/buckets-real-full \
        --out artifacts/levers-bp/real-full-v3/robust \
        --threads 4 --thread-mode hogwild \
        --dcfr-alpha 1.5 --dcfr-beta 0.0
